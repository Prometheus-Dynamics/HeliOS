#!/usr/bin/env python3
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import ota_release_publisher as publisher


class OtaReleasePublisherTests(unittest.TestCase):
    def test_default_post_url_frontend_from_v1(self) -> None:
        self.assertEqual(
            publisher.default_post_url("http://172.31.250.1/v1", "frontend_bundle"),
            "http://172.31.250.1/",
        )

    def test_default_post_url_service_bundle_stays_on_api(self) -> None:
        self.assertEqual(
            publisher.default_post_url("http://172.31.250.1/v1", "service_bundle"),
            "http://172.31.250.1/v1",
        )

    def test_build_bundle_archive_supports_full_tree_entry(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source_dir = root / "source"
            source_dir.mkdir()
            (source_dir / "index.html").write_text("hello", encoding="utf-8")
            (source_dir / "assets").mkdir()
            (source_dir / "assets" / "app.js").write_text("console.log('hi')", encoding="utf-8")
            bundle_path = root / "frontend.tar"

            publisher.build_bundle_archive(source_dir, bundle_path, ["."])

            self.assertTrue(bundle_path.exists())
            with publisher.tarfile.open(bundle_path, "r") as archive:
                names = set(archive.getnames())
            self.assertIn("./index.html", names)
            self.assertIn("./assets/app.js", names)

    def test_build_bundle_archive_supports_specific_entries(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source_dir = root / "source"
            source_dir.mkdir()
            (source_dir / "helios-api").write_text("bin", encoding="utf-8")
            (source_dir / "helios-engine").write_text("bin", encoding="utf-8")
            bundle_path = root / "service.tar"

            publisher.build_bundle_archive(source_dir, bundle_path, ["helios-api"])

            with publisher.tarfile.open(bundle_path, "r") as archive:
                names = archive.getnames()
            self.assertEqual(names, ["helios-api"])

    def test_wait_for_apply_completion_accepts_updater_restart_to_idle(self) -> None:
        states = iter(
            [
                publisher.StateSnapshot(update_id="u1", stage="staged", last_error=None, raw={}),
                publisher.StateSnapshot(update_id="u1", stage="applying", last_error=None, raw={}),
                publisher.StateSnapshot(update_id=None, stage=None, last_error=None, raw={}),
            ]
        )

        publisher.wait_for_apply_completion(
            "u1",
            "Service bundle",
            5.0,
            lambda: next(states, None),
            now=self._monotonic_counter(),
            sleep_fn=lambda _: None,
        )

    def test_wait_for_apply_completion_raises_on_rollback(self) -> None:
        states = iter(
            [
                publisher.StateSnapshot(
                    update_id="u1",
                    stage="rolled_back",
                    last_error="healthcheck failed",
                    raw={},
                )
            ]
        )

        with self.assertRaisesRegex(publisher.PublishError, "rolled back"):
            publisher.wait_for_apply_completion(
                "u1",
                "Service bundle",
                5.0,
                lambda: next(states, None),
                now=self._monotonic_counter(),
                sleep_fn=lambda _: None,
            )

    def test_build_publish_spec_defaults_entries_and_post_url(self) -> None:
        args = publisher.parse_args(
            [
                "publish",
                "--artifact-kind",
                "service_bundle",
                "--base-url",
                "http://172.31.250.1/v1",
                "--source-dir",
                ".",
            ]
        )

        spec = publisher.build_publish_spec(args)

        self.assertEqual(spec.entries, (".",))
        self.assertEqual(spec.post_url, "http://172.31.250.1/v1")
        self.assertTrue(spec.delete_image_after_apply)

    @staticmethod
    def _monotonic_counter() -> callable:
        current = {"value": 0.0}

        def inner() -> float:
            current["value"] += 0.5
            return current["value"]

        return inner


if __name__ == "__main__":
    unittest.main()
