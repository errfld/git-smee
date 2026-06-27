import tempfile
import unittest
from pathlib import Path

import update_homebrew_formula


CHECKSUMS = {
    "git-smee-v1.2.3-aarch64-apple-darwin.tar.gz": "a" * 64,
    "git-smee-v1.2.3-x86_64-apple-darwin.tar.gz": "b" * 64,
    "git-smee-v1.2.3-x86_64-unknown-linux-gnu.tar.gz": "c" * 64,
}


class HomebrewFormulaRenderingTests(unittest.TestCase):
    def test_render_formula_includes_each_supported_platform_url_and_checksum(self):
        formula = update_homebrew_formula.render_formula("v1.2.3", CHECKSUMS)

        self.assertIn('version "1.2.3"', formula)
        self.assertIn("on_macos do", formula)
        self.assertIn("on_linux do", formula)
        self.assertIn("on_arm do", formula)
        self.assertIn("on_intel do", formula)

        for asset, checksum in CHECKSUMS.items():
            self.assertIn(
                f"https://github.com/errfld/git-smee/releases/download/v1.2.3/{asset}",
                formula,
            )
            self.assertIn(f'sha256 "{checksum}"', formula)

    def test_load_checksums_rejects_missing_targets(self):
        with tempfile.TemporaryDirectory() as tmp:
            checksum_dir = Path(tmp)
            (checksum_dir / "git-smee-v1.2.3-aarch64-apple-darwin.tar.gz.sha256").write_text(
                f"{'a' * 64}  git-smee-v1.2.3-aarch64-apple-darwin.tar.gz\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "missing Homebrew checksum assets"):
                update_homebrew_formula.load_checksums("v1.2.3", checksum_dir)

    def test_version_from_tag_matches_release_workflow_suffixes(self):
        accepted_tags = ["v1.2.3-rc-1", "v1.2.3-beta+build5", "v1.2.3-beta_1"]

        for tag in accepted_tags:
            with self.subTest(tag=tag):
                self.assertEqual(tag.removeprefix("v"), update_homebrew_formula.version_from_tag(tag))


if __name__ == "__main__":
    unittest.main()
