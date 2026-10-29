"""Exercise inherited-risk classification, provenance and scanner coverage failures."""

import copy
import hashlib
import json
import unittest

from check_upstream_security import classify, official_lock, validate_policy


class UpstreamSecurityTests(unittest.TestCase):
    def setUp(self):
        self.package = {
            "name": "example",
            "version": "1.0.0",
            "source": "registry+https://example.test",
            "checksum": "a",
        }
        self.finding = {
            "type": "diagnostic",
            "fields": {
                "severity": "error",
                "advisory": {
                    "id": "RUSTSEC-2099-9999",
                    "package": "example",
                    "title": "New inherited risk",
                },
                "graphs": [{"Krate": {"name": "example", "version": "1.0.0"}}],
            },
        }
        self.summary = {"type": "summary", "fields": {"advisories": {"errors": 1}}}

    def result(self, package=None, diagnostics=None, status=1):
        return classify(
            diagnostics or [self.finding, self.summary],
            [package or self.package],
            [self.package],
            status,
        )

    def test_new_inherited_id_needs_no_new_user_approval_but_remains_unfixed(self):
        report = self.result()
        self.assertEqual(report["rejected"], [])
        self.assertEqual(report["scannerExit"], 1)
        self.assertFalse(report["acceptedUpstreamRisks"][0]["riskFixed"])

    def test_fork_source_or_checksum_changes_are_not_inherited(self):
        for key in ("source", "checksum", "version"):
            package = copy.deepcopy(self.package)
            package[key] = "different"
            with self.subTest(key=key):
                self.assertTrue(self.result(package)["rejected"])

    def test_ambiguous_package_identity_is_rejected(self):
        other = dict(self.package, source="git+https://example.test")
        self.assertTrue(
            classify(
                [self.finding, self.summary], [self.package, other], [self.package], 1
            )["rejected"]
        )

    def test_missing_summary_uncovered_error_and_abnormal_exit_fail(self):
        for diagnostics, status in (
            ([self.finding], 1),
            ([self.summary], 1),
            ([self.finding, self.summary], 101),
        ):
            with self.subTest(status=status, diagnostics=diagnostics):
                self.assertTrue(
                    self.result(diagnostics=diagnostics, status=status)["rejected"]
                )

    def test_index_failure_is_not_waived_as_upstream_risk(self):
        failure = {
            "type": "diagnostic",
            "fields": {"severity": "warning", "code": "index-failure"},
        }
        self.assertTrue(
            self.result(diagnostics=[self.finding, failure, self.summary])["rejected"]
        )

    def test_an_unlabelled_error_cannot_hide_behind_a_matching_advisory(self):
        failure = {
            "type": "diagnostic",
            "fields": {"severity": "error", "code": "other"},
        }
        self.assertTrue(
            self.result(diagnostics=[self.finding, failure, self.summary])["rejected"]
        )

    def test_official_lock_hash_cargo_base_and_tag_are_bound(self):
        data = b'[[package]]\nname="example"\nversion="1.0.0"\n'
        policy = {
            "baseVersion": "0.165.0",
            "officialCommit": "a" * 40,
            "officialLockSha256": hashlib.sha256(data).hexdigest(),
        }
        self.assertEqual(
            validate_policy(
                policy, "0.165.0", data, "refs/tags/codex-v0.165.0-deepseek.2"
            )[0]["name"],
            "example",
        )
        for version, payload, ref in (
            ("0.166.0", data, ""),
            ("0.165.0", data + b"#modified", ""),
            ("0.165.0", data, "refs/tags/codex-v0.164.0-deepseek.1"),
        ):
            with self.subTest(version=version, ref=ref), self.assertRaises(ValueError):
                validate_policy(policy, version, payload, ref)

    def test_a_main_commit_cannot_replace_official_stable_provenance(self):
        policy = {"baseVersion": "0.165.0", "officialCommit": "a" * 40}

        def fetch(_):
            return json.dumps({"object": {"type": "commit", "sha": "b" * 40}}).encode()

        with self.assertRaises(ValueError):
            official_lock(policy, fetch)


if __name__ == "__main__":
    unittest.main()
