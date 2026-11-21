import json
import os
import signal
import time
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import dominion_autopilot as a


def git(repo, *args):
    return subprocess.run(['git', '-C', str(repo), *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout.strip()


class AutopilotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='dominion-autopilot-test-')
        self.root = Path(self.temp.name)
        self.repo = self.root / 'watched'
        self.remote = self.root / 'remote.git'
        self.state = self.root / 'state'
        self.repo.mkdir()
        git(self.repo, 'init', '-b', 'main')
        git(self.repo, 'config', 'user.email', 'test@example.invalid')
        git(self.repo, 'config', 'user.name', 'Test')
        (self.repo / 'README.md').write_text('untouched watched checkout\n')
        git(self.repo, 'add', 'README.md')
        git(self.repo, 'commit', '-m', 'init')
        for branch in a.BRANCHES[1:]:
            git(self.repo, 'branch', branch)
        git(self.root, 'init', '--bare', str(self.remote))
        git(self.repo, 'remote', 'add', 'origin', str(self.remote))
        for branch in a.BRANCHES:
            git(self.repo, 'push', 'origin', f'{branch}:{branch}')
        self.original_head = git(self.repo, 'rev-parse', 'HEAD')
        self.agent = self.root / 'fake_agent.py'
        self.agent.write_text('''\
import os, pathlib, subprocess, sys
branch=os.environ['DOMINION_BRANCH']
if os.environ.get('FAKE_NO_COMMIT') == branch:
 print('No actionable changes')
 sys.exit(0)
path=pathlib.Path('AUTOPILOT.txt')
if path.exists(): path=pathlib.Path('AUTOPILOT-2.txt')
path.write_text(branch + '\\n')
subprocess.run(['git','add',str(path)],check=True)
subprocess.run(['git','-c','user.name=Bot','-c','user.email=bot@example.invalid',
                'commit','-m','test agent change'],check=True)
print('Committed',branch)
''')
        self.judge = self.root / 'fake_judge.py'
        self.judge.write_text('''\
import json, sys
print(json.dumps({'summary':'code review only','feedback':{
    'main':'Fix main-specific next issue','dominion-sub2':'Improve sub2',
    'dominion-sub3':'Improve sub3','dominion-sub4':'Improve sub4'},
    'verified':[],'missing_evidence':['live run'],'recommendations':[]}))
''')
        self.command = f'{sys.executable} {self.agent} {{prompt}}'
        self.judge_command = f'{sys.executable} {self.judge} {{prompt}}'

    def tearDown(self):
        self.temp.cleanup()

    def test_all_four_run_without_mutating_watched_checkout(self):
        code = a.main(['--repo', str(self.repo), '--state', str(self.state),
                       '--agent-command', self.command, '--judge-command', self.judge_command,
                       '--cooldown', '0'])
        self.assertEqual(code, 0)
        result = json.loads((self.state/'latest_round.json').read_text())
        self.assertEqual(len(result['results']),4)
        self.assertEqual([x['status'] for x in result['results']], ['committed_local']*4)
        self.assertEqual(git(self.repo, 'rev-parse', 'HEAD'), self.original_head)
        self.assertEqual(git(self.repo, 'status', '--porcelain'), '')
        report = json.loads((self.state/'runs'/result['id']/'judge'/'report.json').read_text())
        self.assertEqual(report['status'],'judge_feedback_recorded')
        self.assertEqual(len(report['feedback']), 4)
        for r in result['results']:
            self.assertEqual(git(self.remote,'rev-parse',f'refs/heads/{r["branch"]}'),self.original_head)
            self.assertTrue(Path(r['worktree']).joinpath('AUTOPILOT.txt').is_file())

    def test_publish_is_opt_in_and_fast_forward(self):
        code=a.main(['--repo',str(self.repo),'--state',str(self.state),
                     '--agent-command',self.command,'--skip-judge','--publish','--cooldown','0'])
        self.assertEqual(code,0)
        result=json.loads((self.state/'latest_round.json').read_text())
        self.assertEqual([x['status'] for x in result['results']],['published']*4)
        for r in result['results']:
            self.assertEqual(git(self.remote,'rev-parse',f'refs/heads/{r["branch"]}'),r['head'])
        self.assertEqual(git(self.repo,'rev-parse','HEAD'), self.original_head)

    def test_second_round_uses_judge_feedback(self):
        self.assertEqual(a.main(['--repo',str(self.repo),'--state',str(self.state),
                     '--agent-command',self.command,'--judge-command',self.judge_command,
                     '--rounds','2','--cooldown','0']),0)
        latest=json.loads((self.state/'latest_round.json').read_text())
        prompt=Path(self.state/'runs'/latest['id']/'main'/'prompt.txt').read_text()
        self.assertIn('Fix main-specific next issue',prompt)
        self.assertTrue((Path(next(r['worktree'] for r in latest['results'] if r['branch']=='main'))/'AUTOPILOT-2.txt').exists())
        self.assertTrue(all(r['status']=='committed_local' for r in latest['results']))

    def test_dirty_worktree_is_not_auto_published(self):
        self.agent.write_text("import pathlib; pathlib.Path('dirty.txt').write_text('draft')\n")
        self.assertEqual(a.main(['--repo',str(self.repo),'--state',str(self.state),
                     '--agent-command',self.command,'--skip-judge','--publish','--cooldown','0']),0)
        latest=json.loads((self.state/'latest_round.json').read_text())
        self.assertTrue(all(r['status']=='needs_review_uncommitted' for r in latest['results']))
        self.assertEqual(git(self.remote,'rev-parse','refs/heads/main'),self.original_head)




    def test_dirty_watched_main_prevents_main_publish_only(self):
        (self.repo/'user-ui-work.tsx').write_text('precious uncommitted change')
        self.assertEqual(a.main(['--repo',str(self.repo),'--state',str(self.state),
                                 '--agent-command',self.command,'--skip-judge',
                                 '--publish','--cooldown','0']),0)
        results=json.loads((self.state/'latest_round.json').read_text())['results']
        self.assertEqual(results[0]['status'],'needs_review_watched_checkout_dirty')
        self.assertEqual([r['status'] for r in results[1:]], ['published']*3, results)
        self.assertEqual(git(self.remote,'rev-parse','refs/heads/main'),self.original_head)
        self.assertTrue((self.repo/'user-ui-work.tsx').exists())

    def test_repeated_noop_does_not_lose_unpushed_local_history(self):
        self.agent.write_text("""import pathlib,subprocess,os
p=pathlib.Path('ONLY-FIRST.txt')
if not p.exists():
 p.write_text('local')
 subprocess.run(['git','add',str(p)],check=True)
 subprocess.run(['git','-c','user.name=Bot','-c','user.email=bot@example.invalid','commit','-m','one'],check=True)
""")
        self.assertEqual(a.main(['--repo',str(self.repo),'--state',str(self.state),
                       '--agent-command',self.command,'--skip-judge','--rounds','3','--cooldown','0']),0)
        results=json.loads((self.state/'latest_round.json').read_text())['results']
        self.assertTrue(all(r['status']=='committed_local_no_new_changes' for r in results))
        self.assertTrue(all(Path(r['worktree'],'ONLY-FIRST.txt').exists() for r in results))
        self.assertEqual(git(self.remote,'rev-parse','refs/heads/main'),self.original_head)

    def test_sigint_stops_running_agents_instead_of_waiting_for_timeout(self):
        self.agent.write_text('import time; time.sleep(120)\n')
        command=[sys.executable,str(ROOT/'tools'/'dominion_autopilot.py'),
                 '--repo',str(self.repo),'--state',str(self.state),
                 '--agent-command',self.command,'--skip-judge','--rounds','0','--cooldown','0',
                 '--timeout','300']
        proc=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        try:
            # Wait for all worktrees to be launched, then signal the coordinator.
            deadline=time.time()+10
            while time.time()<deadline and not (self.state/'worktrees').exists():
                time.sleep(0.1)
            time.sleep(1)
            proc.send_signal(signal.SIGINT)
            stdout,stderr=proc.communicate(timeout=12)
            self.assertEqual(proc.returncode,130,(stdout,stderr))
            self.assertIn('stopped by user',stderr)
        finally:
            if proc.poll() is None:
                proc.kill();proc.communicate()

    def test_sensitive_commit_blocks_automatic_publish(self):
        self.agent.write_text("import pathlib,subprocess\npathlib.Path('.env').write_text('key=x')\nsubprocess.run(['git','add','-f','.env'],check=True)\nsubprocess.run(['git','-c','user.name=Bot','-c','user.email=bot@example.invalid','commit','-m','unsafe'],check=True)\n")
        self.assertEqual(a.main(['--repo',str(self.repo),'--state',str(self.state),
                     '--agent-command',self.command,'--skip-judge','--publish','--cooldown','0']),0)
        results=json.loads((self.state/'latest_round.json').read_text())['results']
        self.assertTrue(all(r['status']=='needs_review_sensitive_paths' for r in results))
        self.assertTrue(all(r['risky_paths']==['.env'] for r in results))
        self.assertEqual(git(self.remote,'rev-parse','refs/heads/main'),self.original_head)


    def test_shared_owner_goal_is_sent_once_to_every_agent_and_judge(self):
        goal='Implement source-cited continuation in Chat and Room'
        self.assertEqual(a.main(['--repo',str(self.repo),'--state',str(self.state),
                       '--agent-command',self.command,'--judge-command',self.judge_command,
                       '--goal',goal,'--cooldown','0']),0)
        run=json.loads((self.state/'latest_round.json').read_text())
        for branch in a.BRANCHES:
            prompt=(self.state/'runs'/run['id']/branch/'prompt.txt').read_text()
            self.assertIn(goal,prompt)
        judge_prompt=(self.state/'runs'/run['id']/'judge'/'prompt.txt').read_text()
        self.assertIn(goal,judge_prompt)

    def test_singleton_lock(self):
        path=self.state/'autopilot.lock'
        with a.singleton_lock(path):
            with self.assertRaises(RuntimeError):
                with a.singleton_lock(path):
                    pass

    def test_state_inside_repo_rejected(self):
        with self.assertRaises(SystemExit):
            a.main(['--repo',str(self.repo),'--state',str(self.repo/'state'),
                    '--agent-command',self.command,'--skip-judge'])

    def test_command_template_does_not_use_shell(self):
        with self.assertRaises(ValueError):
            a.command_from_template('echo run','instruction')
        parts=a.command_from_template('echo {prompt}', 'hello; rm -rf foo')
        self.assertEqual(parts, ['echo','hello; rm -rf foo'])

if __name__ == '__main__': unittest.main()