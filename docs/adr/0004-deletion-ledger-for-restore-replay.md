# Keep a deletion ledger so a database restore cannot resurrect erased accounts

> Superseded by the [local-only spec](../../.scratch/local-only/spec.md) (2026-10-20): the app runs entirely on the user's machine and has no accounts, and the Worker that carried this out is removed. Kept as history.

Account deletion is meant to be permanent once the 30-day recovery period ends, and the screen promises the account is gone. Cloudflare D1's Time Travel point-in-time recovery is always on and cannot be shortened, so an operator restore inside the platform's retention window returns erased rows, including session rows, exactly as they were. The application therefore records a minimal ledger for each completed erasure — a keyed digest of the email address and the erasure timestamp, no account data — and any restore is followed by replaying those erasures before the system returns to service. Each ledger row is dropped once the platform can no longer reach the data it describes.
