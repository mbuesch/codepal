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
- Implementation errors, usage errors or systematic errors in cryptographic algorithms or protocols.
- Usage of broken cryptographic algorithms or protocols for security sensitive tasks.

Also assess existing security measures and identify areas for improvement.
Do not run the software build, linters or tests.

Perform a security audit of: