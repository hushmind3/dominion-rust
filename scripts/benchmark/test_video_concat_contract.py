"""Actual offline FFmpeg concat smoke, not paid video generation or a Rust test.

This produces two synthetic 3-second, audio-bearing clips, runs the same
concat demuxer flags used by DOMINION, and verifies a complete 6-second MP4.
It is deliberately tiny and skips on machines without FFmpeg / FFprobe.
"""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


@unittest.skipUnless(shutil.which("ffmpeg") and shutil.which("ffprobe"),
                     "FFmpeg and FFprobe required for media integration smoke")
class FilmConcatSmoke(unittest.TestCase):
    def command(self, *args):
        return subprocess.run(list(args), capture_output=True, text=True,
                              check=True, timeout=45)

    def probe(self, path):
        result = self.command(
            "ffprobe", "-v", "error", "-show_entries",
            "format=duration:stream=codec_type,width,height,duration",
            "-of", "json", str(path))
        return json.loads(result.stdout)

    def test_concat_keeps_both_paid_segments_and_audio(self):
        with tempfile.TemporaryDirectory(prefix="dominion-film-contract-") as temp:
            root = Path(temp)
            for color in ("blue", "red"):
                self.command(
                    "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
                    "-f", "lavfi", "-i",
                    f"color=c={color}:s=320x240:r=10:d=3",
                    "-f", "lavfi", "-i", "anullsrc=r=8000:cl=mono",
                    "-t", "3", "-shortest",
                    "-c:v", "mpeg4", "-q:v", "5",
                    "-c:a", "aac", "-b:a", "32k", str(root / f"{color}.mp4"))
            playlist = root / "clips.txt"
            playlist.write_text(
                "file 'blue.mp4'\nfile 'red.mp4'\n", encoding="utf-8")
            staged = root / "output.part.mp4"
            self.command(
                "ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
                "-f", "concat", "-safe", "0", "-i", str(playlist),
                "-c", "copy", "-movflags", "+faststart", str(staged))
            report = self.probe(staged)
            duration = float(report["format"]["duration"])
            streams = {stream["codec_type"] for stream in report["streams"]}
            self.assertTrue({"audio", "video"}.issubset(streams))
            self.assertLessEqual(abs(duration - 6.0), max(1.0, 6.0 * .05))
            # A single valid 3-second clip is not a valid 6-second film.
            incomplete = float(self.probe(root / "blue.mp4")["format"]["duration"])
            self.assertGreater(abs(incomplete - 6.0), max(1.0, 6.0 * .05))


if __name__ == "__main__":
    unittest.main()
