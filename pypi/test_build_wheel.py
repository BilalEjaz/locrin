import os, stat, subprocess, sys, tempfile, unittest, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))

class BuildWheel(unittest.TestCase):
    def build(self, tag, exe=False):
        tmp = tempfile.mkdtemp()
        binary = os.path.join(tmp, "locrin.exe" if exe else "locrin")
        with open(binary, "wb") as f:
            f.write(b"#!/bin/sh\necho locrin 9.9.9\n")
        subprocess.run([sys.executable, os.path.join(HERE, "build_wheel.py"), "--version", "9.9.9",
                        "--binary", binary, "--platform-tag", tag, "--out", tmp], check=True)
        return os.path.join(tmp, f"locrin-9.9.9-py3-none-{tag}.whl")

    def test_layout_and_metadata(self):
        whl = self.build("manylinux_2_35_x86_64")
        self.assertTrue(os.path.exists(whl))
        with zipfile.ZipFile(whl) as z:
            names = set(z.namelist())
            self.assertEqual(names, {
                "locrin/__init__.py", "locrin/__main__.py", "locrin/bin/locrin",
                "locrin-9.9.9.dist-info/METADATA", "locrin-9.9.9.dist-info/WHEEL",
                "locrin-9.9.9.dist-info/entry_points.txt", "locrin-9.9.9.dist-info/RECORD",
            })
            meta = z.read("locrin-9.9.9.dist-info/METADATA").decode()
            self.assertIn("Name: locrin\n", meta)
            self.assertIn("Version: 9.9.9\n", meta)
            self.assertIn("Requires-Python: >=3.9\n", meta)
            wheel = z.read("locrin-9.9.9.dist-info/WHEEL").decode()
            self.assertIn("Root-Is-Purelib: false\n", wheel)
            self.assertIn("Tag: py3-none-manylinux_2_35_x86_64\n", wheel)
            ep = z.read("locrin-9.9.9.dist-info/entry_points.txt").decode()
            self.assertIn("locrin = locrin.__main__:main", ep)
            record = z.read("locrin-9.9.9.dist-info/RECORD").decode().splitlines()
            self.assertEqual(len(record), 7)
            self.assertIn("locrin-9.9.9.dist-info/RECORD,,", record)
            self.assertTrue(any(l.startswith("locrin/bin/locrin,sha256=") for l in record))
            info = z.getinfo("locrin/bin/locrin")
            self.assertEqual((info.external_attr >> 16) & 0o111, 0o111)
            self.assertEqual(info.create_system, 3)
            self.assertTrue(stat.S_ISREG(info.external_attr >> 16))

    def test_windows_binary_name(self):
        whl = self.build("win_amd64", exe=True)
        with zipfile.ZipFile(whl) as z:
            self.assertIn("locrin/bin/locrin.exe", z.namelist())

    def test_main_locates_binary(self):
        sys.path.insert(0, HERE)
        import locrin.__main__ as m
        self.assertTrue(m.binary().endswith(os.path.join("bin", "locrin.exe" if sys.platform == "win32" else "locrin")))

if __name__ == "__main__":
    unittest.main()
