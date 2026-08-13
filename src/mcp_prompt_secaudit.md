Perform a **security audit**.
A security audit is a *deep systematic evaluation* of the security of a system or application.

Find vulnerabilities, weaknesses, and potential attack vectors in:
The source code, the software and system design, the architecture, etc... .

Things that are considered to be critical security vulnerabilities include, but are not limited to:
- Triggering of Undefined Behavior (UB).
- Integer overflows triggered by external input.
- Panics and crashes triggered by external input.
- Unintended privilege escalation.
- Unintended private information disclosure: Private keys, secrets, credentials, internal-only information, etc... .
- Denial of Service (DoS).
- TOCTOU (Time of Check to Time of Use) bugs in security critical contexts.
- Possible timing attacks in security critical contexts. (Constant time algorithms/compare).
- Implementation errors, usage errors or systematic errors in cryptographic algorithms or protocols.
- Usage of broken cryptographic algorithms or protocols for security sensitive tasks.

Also assess existing security measures and identify areas for improvement.
Do not run the software build, linters or tests.

Write a detailed report to the file `SECURITY_AUDIT_YYYY-MM-DD_hh-mm-ss.md` where `YYYY-MM-DD_hh-mm-ss` is the current date and time.
Report format:

```
# Security audit report

- Date: {YYYY-MM-DD}
- Scanned files: {file list}

## Finding {1}: {Brief summary}

- Severity: {High, Medium, Low}
- Files: {affected files}

{Very detailed description}

{Code snippets and code references relevant for understanding the finding}

## Finding {2}: {Brief summary}

...

## Finding {n}: {Brief summary}

...

## Summary

{Table with brief descriptions of the findings}
```

Perform a security audit of:
$(WHAT)
