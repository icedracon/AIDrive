# Third-party notices

`aidrive-api` built with the default `embedded-db` feature contains a PostgreSQL 17 build from [theseus-rs/postgresql-binaries](https://github.com/theseus-rs/postgresql-binaries). On first start it is extracted to the AIDrive data folder (`postgres/server/<version>/`), which also holds the notices of the libraries PostgreSQL itself links (`dependency-notices/`). Rust crate licenses are listed by `cargo deny list`.

## PostgreSQL

```text
PostgreSQL Database Management System
(also known as Postgres, formerly known as Postgres95)

Portions Copyright (c) 1996-2026, PostgreSQL Global Development Group

Portions Copyright (c) 1994, The Regents of the University of California

Permission to use, copy, modify, and distribute this software and its
documentation for any purpose, without fee, and without a written agreement
is hereby granted, provided that the above copyright notice and this
paragraph and the following two paragraphs appear in all copies.

IN NO EVENT SHALL THE UNIVERSITY OF CALIFORNIA BE LIABLE TO ANY PARTY FOR
DIRECT, INDIRECT, SPECIAL, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, INCLUDING
LOST PROFITS, ARISING OUT OF THE USE OF THIS SOFTWARE AND ITS
DOCUMENTATION, EVEN IF THE UNIVERSITY OF CALIFORNIA HAS BEEN ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.

THE UNIVERSITY OF CALIFORNIA SPECIFICALLY DISCLAIMS ANY WARRANTIES,
INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY
AND FITNESS FOR A PARTICULAR PURPOSE.  THE SOFTWARE PROVIDED HEREUNDER IS
ON AN "AS IS" BASIS, AND THE UNIVERSITY OF CALIFORNIA HAS NO OBLIGATIONS TO
PROVIDE MAINTENANCE, SUPPORT, UPDATES, ENHANCEMENTS, OR MODIFICATIONS.
```

## theseus-rs/postgresql-binaries

```text
Copyright (c) 2024, Theseus

Permission to use, copy, modify, and distribute this software and its documentation for any purpose, without fee, and without a written agreement is hereby granted, provided that the above copyright notice and this paragraph and the following two paragraphs appear in all copies.

IN NO EVENT SHALL Theseus BE LIABLE TO ANY PARTY FOR DIRECT, INDIRECT, SPECIAL, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, INCLUDING LOST PROFITS, ARISING OUT OF THE USE OF THIS SOFTWARE AND ITS DOCUMENTATION, EVEN IF Theseus HAS BEEN ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

Theseus SPECIFICALLY DISCLAIMS ANY WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE. THE SOFTWARE PROVIDED HEREUNDER IS ON AN “AS IS” BASIS, AND Theseus HAS NO OBLIGATIONS TO PROVIDE MAINTENANCE, SUPPORT, UPDATES, ENHANCEMENTS, OR MODIFICATIONS.
```

## Microsoft Visual C++ Redistributable (Windows only)

The Windows archive contains `vcruntime140.dll`, `vcruntime140_1.dll` and `msvcp140.dll` from the Microsoft Visual C++ 2015-2022 Redistributable (x64), copied unmodified from the Visual Studio `VC\Redist\MSVC\<version>d\Microsoft.VC143.CRT` folder. They are distributable code under the Microsoft Visual Studio license terms (Distributable Code), deployed app-local so the bundled PostgreSQL starts on a Windows that lacks the runtime. Copyright (c) Microsoft Corporation.
