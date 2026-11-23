"""Use the shared nutrition model, or the explicitly configured neko-core."""

import json
import os
import shlex
import subprocess
import time

from neko_api.core import run
from neko_api.store import default_path

from .animation import act


def request(op, *args):
    override = os.environ.get("NEKO_CORE")
    if override:
        command = [override] if os.path.isfile(override) else shlex.split(override)
        result = subprocess.run([*command, op, *args], capture_output=True, text=True, check=False)
        if op == "line" and result.returncode == 0:
            return {"message": result.stdout.strip()}
        try:
            data = json.loads(result.stdout)
        except ValueError as exc:
            raise ValueError(result.stderr.strip() or "neko-core 回傳無效的資料") from exc
        if result.returncode:
            raise ValueError(data.get("error", result.stdout.strip()))
    else:
        data = run({"op": op, **({"food": args[0]} if args else {})})
    if not data.get("ok", True):
        raise ValueError(data["error"])
    return data


def name():
    try:
        return request("status")["cat"]["name"]
    except (ValueError, OSError, KeyError):
        return "TCSG12"


def feed(food, path):
    result = request("feed", food)
    act("eat", 60, 1, result["cat"]["name"], path)
    return result["message"]


class StatusLine:
    def __init__(self):
        self.text = ""
        self.checked = -float("inf")
        self.saved = None

    def get(self):
        path = default_path()
        try:
            stamp = path.stat().st_mtime_ns
        except FileNotFoundError:
            stamp = None
        if stamp != self.saved or time.monotonic() - self.checked >= 30:
            try:
                line = request("line")["message"]
                self.text = line
                self.saved = path.stat().st_mtime_ns if path.exists() else None
            except (ValueError, OSError):
                self.saved = stamp
            self.checked = time.monotonic()
        return self.text
