import os
import subprocess
import sys


def binary():
    name = "locrin.exe" if sys.platform == "win32" else "locrin"
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), "bin", name)


def main():
    path = binary()
    if not os.path.exists(path):
        sys.stderr.write("locrin: the binary is missing from this installation; reinstall with pip, or see https://github.com/BilalEjaz/locrin#install\n")
        return 2
    args = [path] + sys.argv[1:]
    if sys.platform == "win32":
        return subprocess.call(args)
    os.execv(path, args)


if __name__ == "__main__":
    sys.exit(main())
