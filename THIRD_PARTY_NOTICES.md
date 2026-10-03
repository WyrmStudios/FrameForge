# Third-party notices

FrameForge is licensed under the GPLv3 (see `LICENSE`). The components
below were reimplemented from or derived from other projects and carry their
original notices.

## WFHelper: arbitration run parser and vitus model

`src-tauri/src/arbitration.rs` reimplements the EE.log marker set, run
counting rules and vitus-essence model of WFHelper
(<https://github.com/WFHelper/WFHelper>, `services/arbiRunParser.ts` and
`config/shared/arbiMath.ts`). The vitus model there follows
<https://svesk.github.io/arbi>. The curated EE.log excerpts under
`src-tauri/tests/fixtures/arbitration/` include WFHelper's test fixtures.

```
MIT License

Copyright (c) 2026 WFHelper

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
