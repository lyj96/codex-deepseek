"""Exercise expiry and fail-closed behavior, not scanner-policy downgrades."""

import copy
import unittest

from check_advisory_exceptions import (
    APPROVED,
    BASELINE_IGNORES,
    RELEASE,
    advisory_ids,
    validate_policy,
)


class AdvisoryExceptionTests(unittest.TestCase):
    def setUp(self):
        self.policy = {
            "baseVersion": "0.164.0",
            "releaseTag": RELEASE,
            "entries": [
                {"id": identifier, "package": name, "version": version}
                for identifier, (name, version) in APPROVED.items()
            ],
        }
        self.packages = [
            {"name": name, "version": version} for name, version in APPROVED.values()
        ]
        self.ids = BASELINE_IGNORES | APPROVED.keys()

    def validate(self, **overrides):
        args = {
            "policy": self.policy,
            "workspace_version": "0.164.0",
            "packages": self.packages,
            "audit_ids": self.ids,
            "deny_ids": self.ids,
            "ref": "refs/pull/41/merge",
        }
        args.update(overrides)
        return validate_policy(**args)

    def test_accepts_review_candidate_and_exact_release(self):
        for ref in ("refs/pull/41/merge", f"refs/tags/{RELEASE}"):
            with self.subTest(ref=ref):
                self.assertEqual(self.validate(ref=ref), self.policy["entries"])

    def test_rejects_other_base_and_native_tag(self):
        for args in (
            {"workspace_version": "0.165.0"},
            {"ref": "refs/tags/codex-v0.164.0-deepseek.2"},
            {"ref": "refs/tags/codex-v0.165.0-deepseek.1"},
        ):
            with self.subTest(args=args), self.assertRaises(ValueError):
                self.validate(**args)

    def test_rejects_missing_record_or_mismatched_policies(self):
        for args in (
            {"policy": None},
            {"audit_ids": self.ids - {next(iter(self.ids))}},
        ):
            with self.subTest(args=args), self.assertRaises(ValueError):
                self.validate(**args)

    def test_rejects_unapproved_id_or_package_in_record(self):
        for key, value in (("id", "RUSTSEC-2099-9999"), ("package", "other-crate")):
            policy = copy.deepcopy(self.policy)
            policy["entries"][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.validate(policy=policy)

    def test_requires_rereview_when_locked_versions_change(self):
        packages = copy.deepcopy(self.packages)
        packages[0]["version"] = "99.0.0"
        with self.assertRaises(ValueError):
            self.validate(packages=packages)

    def test_rejects_duplicate_exceptions(self):
        with self.assertRaises(ValueError):
            advisory_ids(["RUSTSEC-2026-0374", {"id": "RUSTSEC-2026-0374"}])

    def test_rejects_an_extra_exception_even_if_both_policies_match(self):
        ids = self.ids | {"RUSTSEC-2099-9999"}
        with self.assertRaises(ValueError):
            self.validate(audit_ids=ids, deny_ids=ids)

    def test_accepts_complete_removal_without_approval_record(self):
        self.assertEqual(
            self.validate(policy=None, audit_ids=set(), deny_ids=set()), []
        )


if __name__ == "__main__":
    unittest.main()
