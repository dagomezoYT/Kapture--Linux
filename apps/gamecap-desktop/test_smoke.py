#!/usr/bin/env python3
"""Quick desktop smoke/perf checks — no recording, no UI window."""

from __future__ import annotations

import statistics
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))


def timed(label: str, fn, loops: int = 50) -> float:
    samples: list[float] = []
    for _ in range(loops):
        t0 = time.perf_counter()
        fn()
        samples.append((time.perf_counter() - t0) * 1000)
    avg = statistics.mean(samples)
    p95 = sorted(samples)[max(int(len(samples) * 0.95) - 1, 0)]
    print(f"  {label:40s}  avg {avg:7.2f} ms   p95 {p95:7.2f} ms")
    return avg


def main() -> int:
    import audio
    import controller
    import library
    import session
    import settings as cfg

    print("GameCap smoke/perf (sin grabar, sin ventana)")
    print("=" * 64)
    fails = 0

    # --- Correctness ---
    cut = session.HUD_CAPTURE_CUT
    region = session._region_without_hud(None)
    print(f"  HUD cut={cut}px  region={region}")
    screen_cmd: list[str] = []
    session._append_capture_target(
        screen_cmd,
        {"capture_mode": "monitor", "mode": "monitor", "monitor_name": "eDP-1"},
    )
    game_cmd: list[str] = []
    session._append_capture_target(
        game_cmd,
        {
            "capture_mode": "game",
            "mode": "game",
            "monitor_name": "eDP-1",
            "_game_region": "1280x720+40+200",
        },
    )
    area_cmd: list[str] = []
    session._append_capture_target(
        area_cmd,
        {
            "capture_mode": "area",
            "mode": "region",
            "monitor_name": "eDP-1",
            "region": "640x480+20+200",
        },
    )
    device_cmd: list[str] = []
    session._append_capture_target(
        device_cmd,
        {"capture_mode": "device", "mode": "device", "v4l2_device": "/dev/video2"},
    )
    screen_region = screen_cmd[screen_cmd.index("--region") + 1]
    game_region = game_cmd[game_cmd.index("--region") + 1]
    area_region = area_cmd[area_cmd.index("--region") + 1]
    if screen_region != region:
        print("  FAIL: Pantalla ya no usa el recorte del monitor")
        fails += 1
    if game_region == screen_region or not game_region.startswith("1280x720+"):
        print(f"  FAIL: Juego grabaría la pantalla ({game_region})")
        fails += 1
    if area_region == screen_region:
        print("  FAIL: Ventana/Región se mezcló con Pantalla")
        fails += 1
    if device_cmd[:2] != ["--device", "/dev/video2"]:
        print(f"  FAIL: Dispositivo cambió ({device_cmd})")
        fails += 1
    import games as games_mod

    if not games_mod.is_launcher_title("Prism Launcher") or not games_mod.is_launcher_title(
        "PrismLauncher"
    ):
        print("  FAIL: el launcher de Minecraft debe ignorarse")
        fails += 1
    if games_mod.is_launcher_title("Minecraft* 26.3") or games_mod.is_launcher_title("Minecraft"):
        print("  FAIL: la ventana del juego no es un launcher")
        fails += 1
    fallback = games_mod.resolve_game_region(None, True, (1920, 1200))
    if fallback != "1920x1200+0+0":
        print(f"  FAIL: sin ventana GL debe usarse el monitor ({fallback})")
        fails += 1
    if games_mod.resolve_game_region(None, False, (1920, 1200)) is not None:
        print("  FAIL: sin juego no debe inventar un recorte")
        fails += 1
    kept = games_mod.resolve_game_region("1600x900+40+80", True, (1920, 1200))
    if kept != "1600x900+40+80":
        print(f"  FAIL: una ventana real del juego no debe sustituirse ({kept})")
        fails += 1
    try:
        session._append_capture_target([], {"capture_mode": "game", "mode": "game", "_game_region": ""})
        print("  FAIL: Juego sin ventana no debe grabar la pantalla")
        fails += 1
    except OSError:
        print("  modos separados OK (juego ≠ pantalla ≠ región ≠ dispositivo)")
    if cut < 80:
        print("  FAIL: HUD cut too small (HUD can leak into video)")
        fails += 1
    if not region.endswith(f"+0+{cut}") and f"+{cut}" not in region:
        # allow game-window adjustments; full monitor must start at cut
        if region.startswith("1920x") or "x" in region:
            parts = region.split("+")
            if len(parts) == 3 and parts[2] != str(cut) and parts[1] == "0":
                # y offset should be cut for full-screen crop
                if int(parts[2]) < cut:
                    print(f"  FAIL: region y={parts[2]} < cut {cut}")
                    fails += 1

    tracks = audio.gsr_audio_tracks("both", True, True, "default_output", "default_input")
    print(f"  audio both -> {tracks}")
    if len(tracks) < 3:
        print("  FAIL: expected 3 tracks (mix, game, mic)")
        fails += 1
    import encode_profile as enc

    legacy = enc.plan_encode(
        "av1", "ultra", 240, True, "1920x1080", enc.LEGACY, frozenset({"av1", "av1_hdr", "av1_10bit"})
    )
    if legacy.fps != 60 or legacy.quality != "high" or legacy.codec != "av1" or legacy.encoder is not None:
        print(f"  FAIL: el perfil VA-API cambió el plan que ya grababa ({legacy})")
        fails += 1
    if legacy.scale != "1920x1080":
        print(f"  FAIL: la escala 1080p no se conservó ({legacy.scale})")
        fails += 1
    nvidia = enc.plan_encode("av1", "ultra", 120, True, "0x0", enc.DISCRETE_NVIDIA, frozenset({"av1", "hevc", "h264"}))
    if nvidia.quality != "ultra" or nvidia.fps != 120 or nvidia.encoder is not None or nvidia.scale is not None:
        print(f"  FAIL: NVENC no aceptó el preset pedido ({nvidia})")
        fails += 1
    if enc.allows_ultra(enc.budget_for("amd", True)) or enc.budget_for("amd", True).name != "vaapi-safe":
        print("  FAIL: la 780M (AMD con panel) no debe ofrecer Ultra")
        fails += 1
    discrete = enc.budget_for("amd", False)
    discrete_plan = enc.plan_encode("av1", "ultra", 180, True, "0x0", discrete, None)
    if (
        not enc.allows_ultra(discrete)
        or discrete.name != "vaapi-discrete"
        or discrete_plan.quality != "ultra"
        or discrete_plan.fps != 120
        or discrete_plan.encoder is not None
    ):
        print(f"  FAIL: una AMD de escritorio debe aceptar Ultra y topar a 120 fps ({discrete_plan})")
        fails += 1
    if not enc.allows_ultra(enc.budget_for("nvidia", True)) or enc.allows_ultra(enc.budget_for("intel", True)):
        print("  FAIL: Ultra debe seguir a la GPU de la pantalla, no al panel Intel")
        fails += 1
    import i18n

    i18n.set_language("en")
    if i18n.t("nav.record") != "Record" or i18n.t("ultra.use") != "Record in Ultra":
        print("  FAIL: el catálogo en inglés no responde")
        fails += 1
    i18n.set_language("es")
    if i18n.t("status.recording_clock", clock="01:02") != "● Grabando 01:02":
        print("  FAIL: el estado en español no se formatea")
        fails += 1
    if i18n.set_language("no-such") != "es":
        print("  FAIL: un idioma desconocido debe volver a español")
        fails += 1
    if set(i18n.ES) != set(i18n.EN):
        print(f"  FAIL: faltan claves de idioma {set(i18n.ES) ^ set(i18n.EN)}")
        fails += 1
    soft = enc.plan_encode("av1", "high", 60, True, "0x0", enc.SOFTWARE, None)
    if soft.codec != "h264" or soft.fps != 30 or soft.encoder != "cpu" or soft.scale != "1280x720":
        print(f"  FAIL: sin GPU de encode debe bajar a H.264 720p30 por CPU ({soft})")
        fails += 1
    if enc.resolve_codec("h264", None) != "h264":
        print("  FAIL: un sondeo ausente no debe reescribir el códec")
        fails += 1
    if enc.resolve_codec("h264", frozenset({"av1", "av1_hdr"})) != "av1":
        print("  FAIL: H.264 pedido en una GPU solo-AV1 debe caer a AV1")
        fails += 1
    vendor, codecs = enc.parse_gsr_info(
        "section=gpu_info\nvendor|amd\nsection=video_codecs\nav1\nav1_hdr\nav1_10bit\nsection=image_formats\njpeg\n"
    )
    if vendor != "amd" or codecs != frozenset({"av1", "av1_hdr", "av1_10bit"}):
        print(f"  FAIL: parseo de gpu-screen-recorder --info ({vendor}, {codecs})")
        fails += 1
    import gpu as gpu_mod

    panel = gpu_mod.Gpu("card1", "/dev/dri/card1", "amd", "AMD", ("eDP-1",), "/dev/dri/renderD128", True)
    discrete = gpu_mod.Gpu("card0", "/dev/dri/card0", "nvidia", "NVIDIA", (), "", False)
    chosen = gpu_mod.choose_record_gpu([discrete, panel])
    if chosen is None or chosen.vendor != "amd":
        print(f"  FAIL: el portátil híbrido debe grabar en la GPU del panel ({chosen})")
        fails += 1
    only_nv = gpu_mod.choose_record_gpu(
        [gpu_mod.Gpu("card0", "/dev/dri/card0", "nvidia", "NVIDIA", ("HDMI-A-1",), "", False)]
    )
    if only_nv is None or only_nv.vendor != "nvidia":
        print(f"  FAIL: un escritorio NVIDIA debe grabar en NVIDIA ({only_nv})")
        fails += 1
    if enc.normalize_audio_bitrate(96) != 192:
        print("  FAIL: 96 kbps must rise to 192 so the editor WAV is not crunchy")
        fails += 1
    if enc.normalize_audio_bitrate(160) != 160 or enc.normalize_audio_bitrate(256) != 256:
        print("  FAIL: explicit 160/256 bitrates were rewritten")
        fails += 1
    if enc.dropped_frames([1 / 60, 3 / 60, 1 / 60], 60) != 2:
        print("  FAIL: a 3-frame gap should count as 2 dropped frames")
        fails += 1
    # 60 real frames stamped on a 1 second soundtrack: safe to renumber onto 60 fps.
    if not enc.should_retime(60, 60, 1.0, 60 * 0.016):
        print("  FAIL: a full 60 fps take should be retimed onto the 1/60 grid")
        fails += 1
    # 10% of the frames missing: renumbering would run the picture ahead of the sound.
    if enc.should_retime(54, 60, 1.0, 0.9):
        print("  FAIL: a short frame count must not be stamped as 60 fps")
        fails += 1
    mic = audio.editor_filter("microfono", {"audio_denoise_mic": True, "audio_denoise_static": True, "audio_denoise_dirty": True})
    game = audio.editor_filter("juego", {"audio_denoise_system": True, "audio_denoise_static": True})
    mix = audio.editor_filter("mezcla", {"audio_denoise_mic": True, "audio_denoise_static": True})
    if "afftdn" not in mic or "alimiter" not in mic or "adeclick" not in mic:
        print(f"  FAIL: mic editor filter missing cleanup ({mic})")
        fails += 1
    if game or mix:
        print(f"  FAIL: game and mix must stay unfiltered (game={game!r} mix={mix!r})")
        fails += 1

    plan = audio.gsr_audio_track_plan("both", True, True, "default_output", "default_input")
    labels = [lab for lab, _ in plan]
    print(f"  audio plan labels -> {labels}")
    if labels != ["mezcla", "juego", "microfono"]:
        print("  FAIL: unexpected track labels")
        fails += 1
    java_plan = audio.gsr_audio_track_plan(
        "both", True, True, "default_output", "default_input", game_app="java"
    )
    if ("juego", "app:java") not in java_plan:
        print(f"  FAIL: Minecraft debe grabar app:java ({java_plan})")
        fails += 1
    mix = next((src for lab, src in java_plan if lab == "mezcla"), "")
    if "app:java" not in mix:
        print(f"  FAIL: la mezcla debe incluir el juego ({mix})")
        fails += 1
    monitor = audio.playback_monitor()
    if monitor and not monitor.endswith(".monitor") and not monitor.startswith("@"):
        print(f"  FAIL: playback_monitor raro ({monitor})")
        fails += 1
    import games as games_live

    game_src = games_live.audio_process_name("minecraft")
    print(f"  audio juego vivo -> {game_src}")
    if game_src and "alsa [" in game_src.lower():
        print(f"  FAIL: el nombre PipeWire ALSA ya no coincide con Flatpak ({game_src})")
        fails += 1

    ctrl = controller.RecordingController()
    snap = ctrl.poll()
    if snap.state != controller.RecordingState.IDLE:
        print(f"  FAIL: idle expected, got {snap.state}")
        fails += 1
    else:
        print(f"  controller idle OK ({snap.message})")

    data = cfg.load()
    assert isinstance(data.get("hud"), bool)
    print(f"  settings load OK (keys={len(data)})")

    files = library.list_recordings(newest_first=True)
    print(f"  library files={len(files)}")

    # --- Perf budgets (idle path used by UI poll) ---
    print("-" * 64)
    print("  budgets: poll < 5ms · region < 2ms · library < 30ms · tracks < 1ms")
    a_poll = timed("controller.poll()", ctrl.poll, 80)
    a_reg = timed("session._region_without_hud(None)", lambda: session._region_without_hud(None), 80)
    a_lib = timed("library.list_recordings()", lambda: library.list_recordings(newest_first=True), 20)
    a_trk = timed(
        "gsr_audio_tracks(both)",
        lambda: audio.gsr_audio_tracks("both", True, True, "default_output", "default_input"),
        200,
    )

    if a_poll > 5:
        print("  FAIL: controller.poll too slow (UI tick bottleneck)")
        fails += 1
    if a_reg > 2:
        print("  WARN: region helper slow (calls gpu-screen-recorder --list-monitors)")
        # Not a hard fail — list_monitors shells out; cache would help later.
    if a_lib > 30:
        print("  FAIL: library listing too slow")
        fails += 1
    if a_trk > 1:
        print("  FAIL: audio track build too slow")
        fails += 1

    # region helper shells out; ensure start_recording path doesn't call it more than needed
    # by checking it's not absurdly slow for a few calls
    if a_reg > 50:
        print("  FAIL: region helper extremely slow")
        fails += 1

    print("=" * 64)
    if fails:
        print(f"RESULT: {fails} fail(s)")
        return 1
    print("RESULT: OK — sin cuellos graves en rutas idle")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
