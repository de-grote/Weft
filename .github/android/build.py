"""Builds the game for 64-bit ARM Android phones into one APK.

    python .github/android/build.py OUT.apk [--validation]

Needs Rust's aarch64-linux-android target, ANDROID_HOME (an SDK with build-tools and a
platform) and the NDK (ANDROID_NDK_LATEST_HOME or ANDROID_NDK_HOME, else the newest under the
SDK's ndk/), and a JDK for signing. Signs with WEFT_KEYSTORE (password WEFT_KEYSTORE_PASSWORD, its only key) or else the
SDK's debug key, made if missing: an APK signed with another key can't update an installed one.

The APK holds libmain.so (crates/android) and every mod's library, and assets/game.zip: the
engine's assets and the mods' folders, unpacked by the game once per version. `--validation`
adds Khronos' validation layer (downloaded once) and makes the APK debuggable, which Android
wants for loading a layer from it; the game then validates, into its log, by itself.
"""

import io
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = "aarch64-linux-android"
MIN_SDK = 26
ABI = "arm64-v8a"
LAYERS = "1.4.363.1"
LAYERS_URL = f"https://github.com/KhronosGroup/Vulkan-ValidationLayers/releases/download/vulkan-sdk-{LAYERS}/android-binaries-{LAYERS}.zip"
LAYER = "libVkLayer_khronos_validation.so"


def newest(dir: Path, pattern: str) -> Path:
    found = [p for p in dir.glob(pattern) if re.search(r"\d", p.name)]
    if not found:
        sys.exit(f"nothing like {pattern} in {dir}")
    return max(found, key=lambda p: [int(n) for n in re.findall(r"\d+", p.name)])


def run(*cmd, **kw):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    subprocess.run([str(c) for c in cmd], check=True, **kw)


def validation_layer(cache: Path) -> bytes:
    archive = cache / f"validation-{LAYERS}.zip"
    if not archive.is_file():
        print(f"downloading {LAYERS_URL}", flush=True)
        part = archive.with_suffix(".part")
        with urllib.request.urlopen(LAYERS_URL) as r, open(part, "wb") as f:
            shutil.copyfileobj(r, f)
        part.replace(archive)
    with zipfile.ZipFile(archive) as z:
        name = next(n for n in z.namelist() if n.endswith(f"{ABI}/{LAYER}"))
        return z.read(name)


def main():
    args = [a for a in sys.argv[1:] if a != "--validation"]
    validation = len(args) < len(sys.argv) - 1
    if len(args) != 1:
        sys.exit(__doc__)
    out = Path(args[0]).resolve()
    sdk = Path(os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT") or Path.home() / "AppData/Local/Android/Sdk")
    ndk_env = os.environ.get("ANDROID_NDK_LATEST_HOME") or os.environ.get("ANDROID_NDK_HOME") or os.environ.get("ANDROID_NDK_ROOT")
    ndk = Path(ndk_env) if ndk_env else newest(sdk / "ndk", "*")
    tools = newest(sdk / "build-tools", "*")
    # A released platform: a preview's (android-37.2, codename DEV) makes an APK only that preview
    # installs.
    stable = [p for p in (sdk / "platforms").glob("android-*") if p.name.split("-", 1)[1].isdigit()]
    if not stable:
        sys.exit(f"no released platform in {sdk / 'platforms'}")
    android_jar = max(stable, key=lambda p: int(p.name.split("-", 1)[1])) / "android.jar"
    windows = platform.system() == "Windows"
    host = {"Windows": "windows-x86_64", "Linux": "linux-x86_64", "Darwin": "darwin-x86_64"}[platform.system()]
    bin = ndk / "toolchains/llvm/prebuilt" / host / "bin"
    clang = bin / f"aarch64-linux-android{MIN_SDK}-clang{'.cmd' if windows else ''}"
    exe = ".exe" if windows else ""
    bat = ".bat" if windows else ""

    # The game's mods: those `weft dist` ships.
    mods = []
    for d in sorted((ROOT / "mods").iterdir()):
        manifest = d / "Cargo.toml"
        if manifest.is_file() and not any(l.replace(" ", "") == "dist=false" for l in manifest.read_text().splitlines()):
            mods.append(d)

    env = dict(os.environ)
    env.update({
        "CC_aarch64_linux_android": str(clang),
        "CXX_aarch64_linux_android": str(clang).replace("clang", "clang++"),
        "AR_aarch64_linux_android": str(bin / f"llvm-ar{exe}"),
        "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER": str(clang),
        # Phones from Android 15 may have 16 KB pages, and older NDKs lay libraries out for 4 KB.
        "CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS": "-C link-arg=-Wl,-z,max-page-size=16384",
        "ANDROID_NDK_HOME": str(ndk),
        # Function names stay for crash reports; line tables would make it four times larger.
        "CARGO_PROFILE_RELEASE_DEBUG": "false",
        "CARGO_PROFILE_RELEASE_STRIP": "debuginfo",
    })
    target_dir = ROOT / "target" / "android"
    packages = ["-p", "weft_android"] + [a for d in mods for a in ("-p", d.name)]
    run("cargo", "build", "--release", "--target", TARGET, "--target-dir", target_dir, *packages, env=env, cwd=ROOT)
    built = target_dir / TARGET / "release"

    version = re.search(r'^version = "(\d+)\.(\d+)\.(\d+)"', (ROOT / "Cargo.toml").read_text(), re.M)
    name = ".".join(version.groups())
    code = int(version[1]) * 10000 + int(version[2]) * 100 + int(version[3])

    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        game = io.BytesIO()
        with zipfile.ZipFile(game, "w", zipfile.ZIP_DEFLATED) as z:
            for f in sorted((ROOT / "crates/engine/assets").rglob("*")):
                if f.is_file():
                    z.write(f, "assets/core/" + f.relative_to(ROOT / "crates/engine/assets").as_posix())
            for d in mods:
                for f in sorted((d / "assets").rglob("*")):
                    if f.is_file():
                        z.write(f, f"mods/{d.name}/" + f.relative_to(d).as_posix())
        (tmp / "assets").mkdir()
        (tmp / "assets/game.zip").write_bytes(game.getvalue())

        # The icon (made from the engine's) and the black theme.
        shutil.copytree(ROOT / ".github/android/res", tmp / "res")
        aapt2 = tools / f"aapt2{exe}"
        run(aapt2, "compile", "--dir", tmp / "res", "-o", tmp / "res.zip")
        unsigned = tmp / "unsigned.apk"
        run(aapt2, "link", "-o", unsigned, "-I", android_jar, "--manifest", ROOT / ".github/android/AndroidManifest.xml",
            "--min-sdk-version", MIN_SDK, "--target-sdk-version", android_jar.parent.name.split("-")[1],
            "--version-code", code, "--version-name", name, "-A", tmp / "assets", tmp / "res.zip",
            *(["--debug-mode"] if validation else []))
        with zipfile.ZipFile(unsigned, "a", zipfile.ZIP_DEFLATED) as z:
            for lib in ["main"] + [d.name for d in mods]:
                z.write(built / f"lib{lib}.so", f"lib/{ABI}/lib{lib}.so")
            if validation:
                z.writestr(f"lib/{ABI}/{LAYER}", validation_layer(target_dir))
        aligned = tmp / "aligned.apk"
        run(tools / f"zipalign{exe}", "-f", "-P", "16", "4", unsigned, aligned)

        keystore = os.environ.get("WEFT_KEYSTORE")
        password = os.environ.get("WEFT_KEYSTORE_PASSWORD", "android")
        if not keystore:
            keystore = Path.home() / ".android" / "debug.keystore"
            password = "android"
            if not keystore.is_file():
                keystore.parent.mkdir(parents=True, exist_ok=True)
                java = Path(os.environ["JAVA_HOME"]) / "bin" if "JAVA_HOME" in os.environ else None
                keytool = shutil.which("keytool") or (java and java / f"keytool{exe}")
                run(keytool, "-genkeypair", "-keystore", keystore, "-storepass", password, "-alias", "androiddebugkey",
                    "-keypass", password, "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000",
                    "-dname", "CN=Android Debug,O=Android,C=US")
            print(f"signing with the debug key {keystore}")
        out.parent.mkdir(parents=True, exist_ok=True)
        run(tools / f"apksigner{bat}", "sign", "--ks", keystore, "--ks-pass", f"pass:{password}", "--out", out, aligned)
    print(f"{out}: Weft {name} for Android")


main()
