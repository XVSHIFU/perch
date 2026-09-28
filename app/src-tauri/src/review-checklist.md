---
name: perch-review-checklist
description: Review a small code change for correctness, data safety, and the smallest useful verification.
---

Read the requested change and relevant code before suggesting edits.
Check observable behavior, error handling, and whether existing user data or credentials could be affected.
Use the project's own conventions. Do not modify unrelated files or upgrade dependencies without a reason.
Run the smallest relevant existing check. Report what was actually verified and any remaining uncertainty.
Never include credentials in logs, examples, or exported configuration.
