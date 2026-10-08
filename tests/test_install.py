"""Host-only fake-root installer contracts; never invokes a device command."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
BINARY = REPO / "target/debug/bootctl-unified"
AIDL = '<manifest><hal format="aidl"><name>android.hardware.boot</name><version>1</version><fqname>IBootControl/default</fqname></hal></manifest>'


class Install(unittest.TestCase):
    def run_install(self, manifest=AIDL, critical=False, successful=True, writable=False):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root = base / "root"
            module = base / "module"
            stubs = base / "stubs"
            stubs.mkdir()
            shutil.copytree(REPO / "module", module)
            def put(path, text, executable=False):
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text)
                if executable:
                    path.chmod(0o755)
            put(root / "vendor/etc/vintf/manifest.xml", manifest)
            put(root / "vendor/etc/init/odd.rc", "service surprising-name /vendor/bin/hw/unrelated-name\n    class early_hal\n" + ("    critical\n" if critical else ""))
            put(root / "vendor/bin/hw/unrelated-name", "ELF android.hardware.boot.IBootControl")
            payload = module / "impl/x86_64"
            payload.mkdir(parents=True)
            shutil.copy(BINARY, payload / "bootctl-unified")
            put(payload / "bootctrl.module.so", "legacy")
            put(stubs / "getprop", '#!/bin/sh\ncase "$1" in ro.product.cpu.abi) echo x86_64 ;; persist.generic_bootctl.rw) echo ' + ("1" if writable else "0") + ' ;; esac\n', True)
            put(stubs / "bootctl", '#!/bin/sh\ncase "$1" in get-current-slot) echo 0 ;; get-number-slots) echo 2 ;; is-slot-marked-successful) exit ' + ("0" if successful else "1") + ' ;; *) exit 90 ;; esac\n', True)
            put(stubs / "chcon", "#!/bin/sh\nexit 0\n", True)
            env = dict(os.environ, PATH=str(stubs) + ":" + os.environ["PATH"], MODPATH=str(module), GENERIC_BOOTCTL_TEST_ROOT=str(root), KSU="true")
            result = subprocess.run(["sh", "-c", 'abort() { echo "$*" >&2; exit 1; }; ui_print() { echo "$*"; }; . "$MODPATH/customize.sh"'], env=env, text=True, capture_output=True, check=False)
            overlay = (module / "system/vendor/bin/hw/unrelated-name").exists()
            backup = (module / "backup/vendor/bin/hw/unrelated-name").exists()
            self.assertEqual((root / "vendor/bin/hw/unrelated-name").read_text(), "ELF android.hardware.boot.IBootControl")
            return result, overlay, backup

    def test_manifest_and_policy_aborts(self):
        cases = [
            dict(manifest="<manifest/>"),
            dict(manifest=AIDL.replace("<version>1", "<version>2")),
            dict(manifest='<manifest><hal format="hidl"><name>android.hardware.boot</name><transport>hwbinder</transport><fqname>@1.2::IBootControl/default</fqname></hal></manifest>'),
            dict(critical=True), dict(successful=False), dict(writable=True),
        ]
        for case in cases:
            with self.subTest(case=case):
                result, overlay, backup = self.run_install(**case)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertFalse(overlay)
                self.assertFalse(backup)

    def test_same_path_without_name_guessing(self):
        result, overlay, backup = self.run_install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(overlay)
        self.assertTrue(backup)


if __name__ == "__main__":
    unittest.main()
