"""RecordingController — UI talks to this, not to shell/ffmpeg directly."""

from __future__ import annotations

import json
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

import i18n
import session


class RecordingState(str, Enum):
    IDLE = "idle"
    PREPARING = "preparing"
    RECORDING = "recording"
    PAUSED = "paused"
    STOPPING = "stopping"
    ERROR = "error"


@dataclass
class Snapshot:
    state: RecordingState
    elapsed_s: float = 0.0
    fps: float = 0.0
    free_gb: float = 0.0
    message: str = ""
    path: str = ""


_STATUS_KEY = {
    RecordingState.IDLE: "status.ready",
    RecordingState.PREPARING: "status.preparing",
    RecordingState.RECORDING: "status.recording",
    RecordingState.PAUSED: "status.paused",
    RecordingState.STOPPING: "status.stopping",
    RecordingState.ERROR: "status.error",
}


class RecordingController:
    """Thin facade over session.py for the wireframe UI."""

    def __init__(self) -> None:
        self._state = RecordingState.IDLE
        self._message = ""
        self._message_key = "status.ready"
        self._error = ""
        self._was_alive = False

    @property
    def state(self) -> RecordingState:
        return self._state

    def status_text(self, elapsed: float = 0.0) -> str:
        if self._state == RecordingState.ERROR and self._error:
            return f"● {self._error}"
        if self._state == RecordingState.RECORDING and elapsed > 0:
            return i18n.t("status.recording_clock", clock=_clock(elapsed))
        if self._state == RecordingState.PAUSED and elapsed > 0:
            return i18n.t("status.paused_clock", clock=_clock(elapsed))
        if self._state == RecordingState.IDLE and self._message_key:
            return i18n.t(self._message_key)
        if self._state == RecordingState.IDLE and self._message:
            return f"● {self._message}"
        return i18n.t(_STATUS_KEY.get(self._state, "status.ready"))

    def toggle(self) -> Snapshot:
        if self._state in {RecordingState.RECORDING, RecordingState.PAUSED, RecordingState.PREPARING}:
            return self.stop()
        return self.start()

    def start(self) -> Snapshot:
        if session.is_recording():
            self._state = RecordingState.RECORDING
            return self.poll()
        self._state = RecordingState.PREPARING
        self._error = ""
        try:
            session.start_recording()
        except OSError as err:
            self._state = RecordingState.ERROR
            self._error = str(err)
            self._message = self._error
            self._message_key = ""
            return self.poll()
        self._state = RecordingState.RECORDING
        self._message_key = "status.recording"
        self._was_alive = True
        return self.poll()

    def stop(self) -> Snapshot:
        self._state = RecordingState.STOPPING
        self._message_key = "status.stopping"
        session.stop_recording()
        return self.poll()

    def pause_toggle(self) -> Snapshot:
        if self._state not in {RecordingState.RECORDING, RecordingState.PAUSED}:
            self._message_key = "status.no_recording"
            return self.poll()
        result = session.toggle_pause()
        if result == "pause":
            self._state = RecordingState.PAUSED
            self._message_key = "status.paused"
        elif result == "resume":
            self._state = RecordingState.RECORDING
            self._message_key = "status.recording"
        else:
            self._message_key = "status.pause_failed"
        return self.poll()

    def poll(self) -> Snapshot:
        alive = session.is_recording()
        elapsed = fps = free_gb = 0.0
        paused = False
        path = ""
        if session.STATUS.is_file():
            try:
                data = json.loads(session.STATUS.read_text(encoding="utf-8"))
                elapsed = float(data.get("elapsed_s") or 0)
                fps = float(data.get("fps") or 0)
                free_gb = float(data.get("free_gb") or 0)
                paused = bool(data.get("paused"))
                path = str(data.get("path") or "")
            except (OSError, json.JSONDecodeError, TypeError, ValueError):
                pass
        try:
            if session.PAUSED.is_file():
                paused = session.PAUSED.read_text(encoding="utf-8").strip() in {"1", "true", "True"}
        except OSError:
            pass

        if self._state == RecordingState.ERROR:
            return Snapshot(self._state, elapsed, fps, free_gb, self.status_text(), path)

        if self._state == RecordingState.STOPPING:
            if alive:
                return Snapshot(self._state, elapsed, fps, free_gb, self.status_text(elapsed), path)
            self._state = RecordingState.IDLE
            self._message_key = "status.saved"
            self._was_alive = False
            return Snapshot(self._state, 0.0, 0.0, free_gb, self.status_text(), path)

        if alive:
            self._was_alive = True
            self._state = RecordingState.PAUSED if paused else RecordingState.RECORDING
            return Snapshot(self._state, elapsed, fps, free_gb, self.status_text(elapsed), path)

        if self._was_alive:
            self._was_alive = False
            session.stop_hud()
            try:
                import audio as audio_mod

                audio_mod.stop_cleanup()
            except Exception:
                pass
            self._state = RecordingState.IDLE
            self._message_key = "status.saved"
            return Snapshot(self._state, 0.0, 0.0, free_gb, self.status_text(), path)

        if self._state not in {RecordingState.IDLE, RecordingState.ERROR}:
            self._state = RecordingState.IDLE
            self._message_key = "status.ready"
        return Snapshot(self._state, 0.0, 0.0, free_gb, self.status_text(), path)

    def set_idle_message(self, text: str) -> None:
        if self._state == RecordingState.IDLE:
            self._message_key = ""
            self._message = text


def _clock(elapsed: float) -> str:
    total = max(int(elapsed), 0)
    hh, rem = divmod(total, 3600)
    mm, ss = divmod(rem, 60)
    if hh:
        return f"{hh}:{mm:02d}:{ss:02d}"
    return f"{mm:02d}:{ss:02d}"
