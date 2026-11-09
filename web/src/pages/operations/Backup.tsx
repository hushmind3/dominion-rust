import {useState} from 'react'
import {Archive, Check, KeyRound, RefreshCw, RotateCcw, ShieldAlert, TriangleAlert} from 'lucide-react'
import {post} from '../../api'
import {Button, TextInput} from '../../components/Primitives'
import {Action, Notice, Panel, StateTag, Subheading, useRemote} from '../../components/UI'
import {t} from '../../i18n'
type Backup = {name: string; path: string; bytes: number; encrypted: boolean}
type KeyStatus = {activeVersion?: number; versions?: number[]; error?: string}
type SecretStatus = {previousKeys?: number; oldKeyFiles?: string[]; error?: string}
type MaintenanceState = {maintenance: boolean}

function formatBytes(bytes?: number) {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`
}

export function BackupPanel() {
  const backups = useRemote<Backup[]>('/api/system/backups', [])
  const backupKeys = useRemote<KeyStatus>('/api/system/backup-keys', {})
  const secretKey = useRemote<SecretStatus>('/api/system/secret-key', {})
  const maintenance = useRemote<MaintenanceState>('/api/system/maintenance', {maintenance: false})
  const [busy, setBusy] = useState('')
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const [selected, setSelected] = useState('')
  const [confirm, setConfirm] = useState(false)
  const [keepOld, setKeepOld] = useState(false)
  const active = maintenance.value.maintenance

  const reload = async () => {
    await Promise.all([backups.reload(), backupKeys.reload(), secretKey.reload(), maintenance.reload()])
  }

  const run = async (name: string, task: () => Promise<void>) => {
    if (busy) return
    setBusy(name)
    setError('')
    setMessage('')
    try {
      await task()
    } catch (cause) {
      setError((cause as Error).message)
    } finally {
      setBusy('')
    }
  }

  const createBackup = () => run('create', async () => {
    const result = await post<{ok: boolean; bytes: number; path: string; remote?: {attempted?: boolean; uploaded?: boolean; verified?: boolean; error?: string}}>('/api/system/backup', {})
    await reload()
    const remote = result.remote
    setMessage(
      remote?.attempted
        ? t('ops.backup.createdRemote', {
            size: formatBytes(result.bytes),
            state: remote.uploaded && remote.verified ? t('ops.backup.remoteVerified') : t('ops.backup.remotePending'),
          })
        : t('ops.backup.created', {size: formatBytes(result.bytes)}),
    )
  })

  const toggleMaintenance = () => run('maintenance', async () => {
    const next = !active
    await post('/api/system/maintenance', {enabled: next})
    await reload()
    setMessage(next ? t('ops.backup.maintenanceOn') : t('ops.backup.maintenanceOff'))
  })

  const restore = () => run('restore', async () => {
    if (!selected || !confirm) return
    await post('/api/system/restore', {path: selected, confirm: true})
    setMessage(t('ops.backup.restored'))
    window.setTimeout(() => window.location.reload(), 1200)
  })

  const rotateBackupKeys = () => run('backupKeys', async () => {
    const result = await post<{activeVersion: number; retainedVersions: number[]; reencrypted: number}>('/api/system/backup-keys/rotate', {keepOld})
    await reload()
    setMessage(t('ops.backup.keysRotated', {version: result.activeVersion, count: result.reencrypted}))
  })

  const rotateSecretKey = () => run('secretKey', async () => {
    const result = await post<{secretsReencrypted: number; legacyBackupsMigrated: number}>('/api/system/secret-key/rotate', {keepOld})
    await reload()
    setMessage(t('ops.backup.secretRotated', {secrets: result.secretsReencrypted, backups: result.legacyBackupsMigrated}))
  })

  return <Panel className="min-w-0 p-5 xl:col-span-2">
    <Subheading title={t('ops.backup')} extra={<Archive size={19}/>}/>
    <p>{t('ops.backup.desc')}</p>
    <div className="mt-3 flex flex-wrap items-center gap-2">
      <Action onClick={createBackup} disabled={Boolean(busy)}>
        {busy === 'create' ? <RefreshCw size={16} className="animate-spin"/> : <Archive size={16}/>} {busy === 'create' ? t('ops.backup.creating') : t('ops.backup.create')}
      </Action>
      <StateTag online={active}>{active ? t('ops.backup.maintenanceActive') : t('ops.backup.maintenanceInactive')}</StateTag>
      <Button variant="secondary" onClick={toggleMaintenance} disabled={Boolean(busy)}>
        <ShieldAlert size={16}/> {active ? t('ops.backup.maintenanceStop') : t('ops.backup.maintenanceStart')}
      </Button>
      <Button variant="ghost" onClick={() => void reload()} disabled={Boolean(busy)}>
        <RefreshCw size={15}/> {t('ops.refresh')}
      </Button>
    </div>
    <div className="mt-4 grid gap-3 sm:grid-cols-2">
      <div className="rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-3">
        <div className="flex items-center justify-between gap-2">
          <strong className="text-[12px] text-[#3A342D]">{t('ops.backup.keys')}</strong>
          <span className="text-[11px] text-[#8A8176]">{t('ops.backup.activeVersion', {version: backupKeys.value.activeVersion ?? 1})}</span>
        </div>
        <p className="mt-1 text-[11px] leading-5 text-[#8A8176]">{t('ops.backup.keys.desc', {count: backupKeys.value.versions?.length ?? 1})}</p>
        <Button variant="secondary" className="mt-2" onClick={rotateBackupKeys} disabled={!active || Boolean(busy)}>
          <KeyRound size={15}/> {busy === 'backupKeys' ? t('ops.backup.rotating') : t('ops.backup.rotateKeys')}
        </Button>
      </div>
      <div className="rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-3">
        <div className="flex items-center justify-between gap-2">
          <strong className="text-[12px] text-[#3A342D]">{t('ops.backup.secret')}</strong>
          <span className="text-[11px] text-[#8A8176]">{t('ops.backup.previousKeys', {count: secretKey.value.previousKeys ?? 0})}</span>
        </div>
        <p className="mt-1 text-[11px] leading-5 text-[#8A8176]">{t('ops.backup.secret.desc')}</p>
        <Button variant="secondary" className="mt-2" onClick={rotateSecretKey} disabled={!active || Boolean(busy)}>
          <KeyRound size={15}/> {busy === 'secretKey' ? t('ops.backup.rotating') : t('ops.backup.rotateSecret')}
        </Button>
      </div>
    </div>
    <label className="mt-3 flex items-center gap-2 text-[11px] text-[#5C544A]">
      <TextInput type="checkbox" checked={keepOld} onChange={event => setKeepOld(event.target.checked)}/>
      {t('ops.backup.keepOld')}
    </label>
    {error && <Notice message={error}/>}
    {message && <div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{message}</div>}
    <div className="mt-5 border-t border-[#EFE9DF] pt-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <strong className="text-[13px] text-[#3A342D]">{t('ops.backup.files')}</strong>
        <span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{backups.value.length} FILES</span>
      </div>
      {backups.value.length ? <div className="mt-2 grid gap-2">
        {backups.value.map(backup => <label key={backup.name} className={'flex min-w-0 flex-wrap items-center gap-3 rounded-xl border px-3 py-2 text-[11px] ' + (selected === backup.name ? 'border-[#C7B18F] bg-[#F2EADF]' : 'border-[#EFE9DF] bg-white')}>
          <TextInput type="radio" name="backup" disabled={!backup.encrypted} checked={selected === backup.name} onChange={() => { setSelected(backup.name); setConfirm(false) }}/>
          <span className="min-w-0 flex-1"><strong className="block break-all text-[11px] text-[#3A342D]">{backup.name}</strong><small className="text-[10px] text-[#9C9388]">{formatBytes(backup.bytes)} · {backup.encrypted ? t('ops.backup.encrypted') : t('ops.backup.plain')}</small></span>
        </label>)}
      </div> : <p className="mt-2 text-[11px] text-[#9C9388]">{t('ops.backup.noFiles')}</p>}
      <div className="mt-3 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-3">
        <div className="flex items-start gap-2 text-[11px] leading-5 text-[#7C5A48]"><TriangleAlert size={15} className="mt-0.5 shrink-0"/><span>{t('ops.backup.restoreWarning')}</span></div>
        <label className="mt-2 flex items-center gap-2 text-[11px] text-[#5C544A]">
          <TextInput type="checkbox" checked={confirm} disabled={!selected || !active} onChange={event => setConfirm(event.target.checked)}/>
          {t('ops.backup.restoreConfirm')}
        </label>
        <Button variant="danger" className="mt-2" disabled={!active || !selected || !confirm || Boolean(busy)} onClick={restore}>
          {busy === 'restore' ? <RefreshCw size={15} className="animate-spin"/> : <RotateCcw size={15}/>} {busy === 'restore' ? t('ops.backup.restoring') : t('ops.backup.restore')}
        </Button>
        {!active && <p className="mt-1 text-[10px] text-[#9A5B48]">{t('ops.backup.needMaintenance')}</p>}
      </div>
    </div>
  </Panel>
}
