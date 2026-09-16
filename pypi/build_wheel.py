"""Build a platform wheel that carries the locrin binary.

Usage: python pypi/build_wheel.py --version 0.6.0 --binary path/to/locrin --platform-tag manylinux_2_35_x86_64 --out dist

No setuptools: a wheel is a zip with a dist-info directory, and writing it
directly keeps the build deterministic and dependency-free.
"""
import argparse
import base64
import hashlib
import os
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
SUMMARY = "Locrin: the deterministic quality gate for code written by people and agents."


def record_line(name, data):
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"{name},sha256={digest},{len(data)}"


def metadata(version):
    with open(os.path.join(HERE, "README.md"), encoding="utf-8") as f:
        readme = f.read()
    return (
        "Metadata-Version: 2.1\n"
        "Name: locrin\n"
        f"Version: {version}\n"
        f"Summary: {SUMMARY}\n"
        "Home-page: https://locrin.com\n"
        "License: MIT\n"
        "Requires-Python: >=3.9\n"
        "Project-URL: Source, https://github.com/BilalEjaz/locrin\n"
        "Description-Content-Type: text/markdown\n"
        "\n" + readme
    )


def build(version, binary, platform_tag, out):
    tag = f"py3-none-{platform_tag}"
    dist_info = f"locrin-{version}.dist-info"
    exe = platform_tag.startswith("win")
    bin_name = "locrin.exe" if exe else "locrin"
    files = []
    for py in ("__init__.py", "__main__.py"):
        with open(os.path.join(HERE, "locrin", py), "rb") as f:
            files.append((f"locrin/{py}", f.read(), 0o644))
    with open(binary, "rb") as f:
        files.append((f"locrin/bin/{bin_name}", f.read(), 0o755))
    files.append((f"{dist_info}/METADATA", metadata(version).encode(), 0o644))
    files.append((f"{dist_info}/WHEEL", f"Wheel-Version: 1.0\nGenerator: locrin-build\nRoot-Is-Purelib: false\nTag: {tag}\n".encode(), 0o644))
    files.append((f"{dist_info}/entry_points.txt", b"[console_scripts]\nlocrin = locrin.__main__:main\n", 0o644))
    record = "\n".join(record_line(n, d) for n, d, _ in files) + f"\n{dist_info}/RECORD,,\n"
    files.append((f"{dist_info}/RECORD", record.encode(), 0o644))

    os.makedirs(out, exist_ok=True)
    path = os.path.join(out, f"locrin-{version}-{tag}.whl")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data, mode in files:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3  # Unix host marker: installers only honour the mode bits when the entry says Unix, whatever host built the wheel
            info.external_attr = (0o100000 | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)
    return path


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--version", required=True)
    p.add_argument("--binary", required=True)
    p.add_argument("--platform-tag", required=True)
    p.add_argument("--out", required=True)
    a = p.parse_args()
    print(build(a.version, a.binary, a.platform_tag, a.out))


if __name__ == "__main__":
    main()
