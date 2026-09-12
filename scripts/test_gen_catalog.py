"""Run with: uv run --with huggingface_hub --with fsspec scripts/test_gen_catalog.py"""
import io
import struct
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import gen_catalog as catalog


def file(name, size=1024, sha="a" * 64):
    return SimpleNamespace(rfilename=name, size=size, lfs=SimpleNamespace(sha256=sha))


class ExternalCatalogTests(unittest.TestCase):
    def test_mixed_repo_includes_only_compatible_export(self):
        wanted = "transcribe-cpp/orukeet-Q8_0.gguf"
        result = catalog.gguf_files("oruk/orukeet", [
            file("orukeet-v0.1.0-q8.gguf"),
            file("orukeet-v0.1.0-f16.gguf"),
            file(wanted),
            file("onnx/manifest.json"),
        ])
        self.assertEqual([f["filename"] for f in result], [wanted])
        self.assertEqual(result[0]["quant"], "Q8_0")

    def test_missing_export_fails_instead_of_falling_back_to_native_gguf(self):
        with self.assertRaisesRegex(ValueError, "missing catalog exports"):
            catalog.gguf_files("oruk/orukeet", [file("orukeet-v0.1.0-q8.gguf")])

    def test_selected_export_requires_integrity_metadata(self):
        for bad in [file("transcribe-cpp/orukeet-Q8_0.gguf", size=None),
                    file("transcribe-cpp/orukeet-Q8_0.gguf", sha=None)]:
            with self.subTest(file=bad):
                with self.assertRaisesRegex(ValueError, "missing/invalid size or sha256"):
                    catalog.gguf_files("oruk/orukeet", [bad])

    def test_existing_publishers_keep_all_quants(self):
        result = catalog.gguf_files("handy-computer/parakeet-tdt-0.6b-v3-gguf", [
            file("parakeet-tdt-0.6b-v3-Q8_0.gguf", size=2048),
            file("parakeet-tdt-0.6b-v3-Q4_K_M.gguf"),
        ])
        self.assertEqual([f["quant"] for f in result], ["Q4_K_M", "Q8_0"])

    def test_external_model_has_no_invented_reference_machine_score(self):
        info = SimpleNamespace(
            sha="b" * 40, tags=["parakeet"],
            card_data=SimpleNamespace(to_dict=lambda: {
                "language": ["en", "de"], "license": "cc-by-sa-4.0",
                "transcribe_cpp": {"lang_detect": True, "timestamps": "token"},
            }),
            siblings=[file("transcribe-cpp/orukeet-Q8_0.gguf")],
        )
        with patch.object(catalog.api, "model_info", return_value=info), \
             patch.object(catalog, "probe_header", return_value={
                 "general.name": "Orukeet", "general.architecture": "parakeet",
                 "general.size_label": "0.6B",
             }):
            model = catalog.build("oruk/orukeet")
        self.assertIsNone(model["speed_score"])
        self.assertIsNone(model["accuracy_score"])
        self.assertEqual(model["default_quant"], "Q8_0")
        self.assertFalse(model["recommended"])

    def test_display_metadata_uses_the_same_revision_as_the_file_hashes(self):
        def string(value):
            encoded = value.encode()
            return struct.pack("<Q", len(encoded)) + encoded
        data = b"GGUF" + struct.pack("<IQQ", 3, 0, 1)
        data += string("general.name") + struct.pack("<I", 8) + string("Orukeet")
        with patch.object(catalog.fs, "open", return_value=io.BytesIO(data)) as opened:
            result = catalog.probe_header("oruk/orukeet", "transcribe-cpp/orukeet-Q8_0.gguf", revision="b" * 40)
        self.assertEqual(result["general.name"], "Orukeet")
        opened.assert_called_once_with("oruk/orukeet@" + "b" * 40 + "/transcribe-cpp/orukeet-Q8_0.gguf", "rb", block_size=65536)


if __name__ == "__main__":
    unittest.main()
