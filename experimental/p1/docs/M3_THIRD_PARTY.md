# M3 dependency and numeric-vector attribution

The crypto experiment links published crates through Cargo, without copying implementation source.
chacha20poly1305 0.10.1 (MIT OR Apache-2.0), zeroize 1.9.1 (Apache-2.0 OR MIT), getrandom 0.4.3
(MIT OR Apache-2.0); exact complete dependency/license inventory is produced by audit_local.py.
These dev-dependencies are not a production provider selection or a licensing change to YixuMemory.

The known-answer test transcribes numeric example data from Appendix A.1 of
https://www.ietf.org/archive/id/draft-irtf-cfrg-xchacha-03.txt (January 10, 2020, expired draft,
not an adopted IETF standard). Copyright (c) 2020 IETF Trust and the persons identified as the
document authors. Author: S. Arciszewski. This is mathematical test data, not copied algorithm code.
For completeness, the IETF code-component Simplified BSD terms are retained below:

Redistribution and use in source and binary forms, with or without modification, are permitted
provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions
   and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of
   conditions and the following disclaimer in the documentation and/or other materials provided
   with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
