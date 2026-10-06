# Changelog

Mọi thay đổi đáng kể của dự án được ghi ở đây. Định dạng theo tinh thần
[Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), phiên bản theo SemVer.

## [0.8.2] – 2026-10-06 — 🧩 "Một chữ `null` không còn làm hỏng cả khối câu hỏi." · "One `null` no longer sinks a whole question block."

Khi thử 0.8.1, Part 4 của một đề HSG báo "Question generation failed" chỉ
vì Gemini viết câu hỏi rỗng của một ô trống thành `null` thay vì `""`.
**0.8.2 đọc được câu trả lời đó, và dặn Gemini đừng viết như vậy nữa.**
*While 0.8.1 was being tested, Part 4 of an HSG exam reported "Question
generation failed" only because Gemini wrote the empty stem of a gap as
`null` instead of `""`. **0.8.2 reads that reply, and tells Gemini not to
write it.***

### English

#### Fixed

- **Gap questions no longer fail on `"stem": null`.** For summary
  completion, note completion and multiple selection the items have no
  question of their own, and Gemini sometimes wrote that empty stem as
  `null`; the whole block then failed with "Question generation failed: …
  invalid type: null, expected a string". A null or missing stem or option
  text now reads as empty, the way empty option lists and evidence already
  did. Nothing slips through: the validator still reports "Empty question"
  where a question type needs a stem and "Option A is empty" for an option
  without text, as issues beside the draft.
- The question prompt gains one rule: an empty string is `""`, an empty list
  is `[]`, never `null`.

#### Inside

- `Item.stem` and `Choice.text` (`src/domain/task.rs`) use the existing
  `null_as_default`; saved exams read and write exactly as before.
- Four tests: the reply that failed (an HSG Part 4 summary completion with
  `"stem": null`), a short answer whose null stem becomes an "Empty question"
  issue, a matching task whose null option text becomes "Option A is empty",
  and stems and option texts that are missing altogether. Without the fix
  all four fail.

#### Known, for a later release

- The validator does not check that note-completion notes contain every gap,
  and options on question types that take none are printed but not checked.
  Both are older than this release.

#### Upgrading

- Nothing to migrate.

### Tiếng Việt

#### Sửa lỗi

- **Câu hỏi có ô trống không còn hỏng vì `"stem": null`.** Với dạng điền tóm
  tắt, điền ghi chú và chọn nhiều đáp án, mỗi câu không có lời hỏi riêng, và
  Gemini đôi khi viết lời hỏi rỗng đó thành `null`; cả khối câu hỏi khi đó báo
  "Question generation failed: … invalid type: null, expected a string". Giờ
  lời hỏi hay nội dung lựa chọn là `null` hoặc thiếu hẳn đều được đọc là rỗng,
  như danh sách lựa chọn rỗng và trích dẫn đã được đọc từ trước. Không gì lọt
  qua: phần kiểm tra vẫn báo "Empty question" ở dạng bài cần lời hỏi và
  "Option A is empty" cho lựa chọn không có nội dung, thành lỗi hiện bên bản
  nháp.
- Prompt câu hỏi thêm một quy tắc: chuỗi rỗng là `""`, danh sách rỗng là
  `[]`, không bao giờ `null`.

#### 🧰 Bên trong

- `Item.stem` và `Choice.text` (`src/domain/task.rs`) dùng `null_as_default`
  sẵn có; đề đã lưu đọc và ghi đúng như cũ.
- Bốn test: đúng câu trả lời đã hỏng (khối điền tóm tắt Part 4 HSG có
  `"stem": null`), một câu trả lời ngắn có lời hỏi `null` thành lỗi "Empty
  question", một bài nối có lựa chọn `null` thành lỗi "Option A is empty", và
  lời hỏi, nội dung lựa chọn thiếu hẳn. Bỏ phần sửa đi thì cả bốn đều hỏng.

#### Biết rồi, để bản sau

- Phần kiểm tra chưa xem ghi chú của dạng điền ghi chú có đủ mọi ô trống
  không, và lựa chọn ở dạng bài không cần lựa chọn được in ra mà không được
  kiểm tra. Cả hai có từ trước bản này.

#### ⚠️ Cần biết khi nâng cấp

- Không cần chuyển đổi gì.

### 📊 Con số biết nói · Numbers

| Kiểm tra · Check | Kết quả · Result |
|---|---|
| `cargo fmt --check` app và launcher · app and launcher | sạch · clean |
| `cargo check` web / server / wasm32 | sạch, **0 warning** cả ba · clean, **0 warnings** on all three |
| `cargo test --features server --no-default-features` | **198/198**, 4 test chạy tay · 4 manual (0.8.1: 194; +4 cho `null` · for `null`) |
| Bỏ phần sửa, chạy lại 4 test mới · New tests without the fix | **4/4 hỏng · fail**, như mong đợi · as they should |
| Part 4 HSG thật trên server dev, cùng chủ đề đã hỏng · Real HSG Part 4 on the dev server, the topic that failed | script 562 từ · words, khối điền tóm tắt (26)–(35) đọc được · summary completion block parsed; 3 lỗi giới hạn từ của chính phần kiểm tra · 3 word-limit issues from the validator itself; **0,009 USD** |
| Bản Windows 0.8.2 · Windows build | `.exe` **9,4 MB** (9.393.152 byte, 0.8.1: 9.394.688); chỉ gọi DLL hệ thống của Windows · Windows system DLLs only |
| `smoke.py` trên chính file `.exe` phát hành · on the release EXE | ✅ đạt · passed; "không có key ở đâu" vẫn **không chạy được** trên máy build · "no key anywhere" still **not run** on the build host. Khối điền tóm tắt Part 4 HSG trên chính `.exe` đó · An HSG Part 4 summary-completion block on that EXE: đủ 10 ô (26)–(35), 0 lỗi · all 10 gaps (26)–(35), 0 issues, **0,0082 USD** |
| Bản Linux 0.8.2 · Linux build | `.AppImage` **13,2 MB** (13.224.440 byte), build trong container Ubuntu 22.04 · built in an Ubuntu 22.04 container; fmt, check và · and 198 test chạy lại trong đó · re-run inside |
| `smoke.py` trên AppImage, Ubuntu 22.04 và 24.04 sạch · on the AppImage, clean Ubuntu 22.04 and 24.04 | ✅ đạt cả hai · passed on both, user `nobody`, kể cả · including **khởi động khi không có key ở đâu · starting with no key anywhere** |
| Tiền Gemini tốn cho 0.8.2 · Gemini spend for 0.8.2 | **0,017 USD**: hai lần sinh khối câu hỏi Part 4 HSG · two HSG Part 4 question runs (server dev · dev server 0,0088, `.exe` phát hành · release EXE 0,0082) (`docs/portable-verification-v0.8.2.md`) |

## [0.8.1] – 2026-10-06 — 📁 "Tên file nói lên nội dung, và tự tải về." · "File names that say what's inside, and save themselves."

Ở 0.8.0, mỗi lần tải về giáo viên nhận `IELTS_Part1_paper.docx`,
`HSG_exam_1a2b3c4d.wav`: mười file trong thư mục Downloads là mười cái tên
không ai nhớ là đề nào. **Từ 0.8.1, tên file cho biết loại đề, nội dung và
lúc tải, và có thể tự tải DOCX lẫn WAV ngay khi xong.**
*In 0.8.0 every download was called `IELTS_Part1_paper.docx` or
`HSG_exam_1a2b3c4d.wav`, so ten files in Downloads were ten names nobody could
place. **From 0.8.1 a file's name tells the test, what it is about and when it
was saved, and the DOCX and the WAV can save themselves as soon as they are
ready.***

### English

#### What's new

- **Download names that say what is inside.** Every DOCX, WAV and Markdown
  download is named `<test type>_<five-word summary>_<DD-MM-YYYY_HH-MM-SS>`,
  for example
  `IELTS-Listening-Part2_Riverside-Community-Garden-Volunteer-Orientation_06-10-2026_16-18-29.docx`
  (the Whole exam page leaves the part out:
  `HSG-Quoc-gia-Listening_Urban-Environment-And-City-Life_06-10-2026_16-25-18.docx`).
  The five words are written by `gemini-3.5-flash-lite` right after a script
  is written, from the exam's theme and the topics of the parts that have a
  script; they cost about $0.00005 a draft and show as "file names" in the
  exam's spend line. The time is the browser's clock, day first. The DOCX and
  the WAV of one draft share a name; new questions, a new recording or a new
  take of something already made get a new time, a first recording keeps the
  questions' name. Vietnamese letters lose their accents (`Hà Nội` becomes
  `Ha-Noi`) and names are valid on Windows, macOS and Linux. If Gemini has not
  answered within ten seconds, the file is named after the topics themselves.
- **Download the DOCX and WAV automatically.** A tick box on both pages, off
  by default and remembered by the browser (every tab follows it within two
  seconds). With it on, the DOCX downloads as soon as the last question block
  is written (of the part, or of the exam's last part, so exactly once per
  exam) and the WAV as soon as the recording is ready, including after
  "Check again". It can be ticked mid-run. Opening a saved exam never
  downloads anything, not even a recording that finishes after it was opened.
  Chrome and Edge ask once whether the site may download several files:
  choose Allow.
- New setting `GEMINI_SUMMARY_MODEL` (default `gemini-3.5-flash-lite`). The
  summary request sends no `thinking_level`, so each model thinks at its own
  default (minimal on Flash-Lite, a level 3.8 Flash refuses).

#### Inside

- `export/naming.rs` (pure: time stamp, accent folding, test type, summary
  and fallback words, stem, file name), `ui/naming.rs` (one `DraftNaming` per
  page: summaries kept with the topics they sum up, the stem fixed at a
  draft's first download, a draft token so a late download never names a
  newer draft, one download per file while it waits), `ui/prefs.rs`
  (`localStorage`), `components/auto_download.rs`.
- Server: `application::naming::summarize_topics`, `GeminiClient::generate_brief`
  (the model named per call, no thinking level, 20 s, 1,024 output tokens,
  same retries and meter), `prompts/summary.rs` (topics quoted as data and
  capped), `UsageStep::Naming`, `Bucket::Naming` (30 a minute), a price for
  3.5 Flash-Lite ($0.30 / $0.03 / $2.50 per million tokens, the same after
  2027), ignored test `brief_live_probe`.
- Downloads: the temporary link is attached to the page while it is clicked
  and blob URLs are revoked after a minute, not at once; the WAV still goes
  straight from `/audio/{job_id}` to disk. The player's WAV link became a
  button that names the file first.

#### Found while testing

- The first real run on the part page stopped with a borrow panic: an
  `if let` kept a read of the page state alive while the summary request
  wrote to it. Fixed before release, and noted in `CLAUDE.md`.
- A review of the change found that a first recording started a new name
  (so the step-by-step DOCX and WAV differed), that a late summary for an
  older exam could push out the current one, that the first download after
  opening an exam waited for its summary, and that the exam's spend line was
  not refreshed after a summary. All four fixed and checked in the browser.
- Not fixed here: a summary-completion question block can come back with
  `"stem": null` on an item, and that block then fails to parse ("Question
  generation failed: … invalid type: null, expected a string"). Seen once on
  HSG Part 4; "Regenerate questions" worked.

#### Upgrading

- Nothing to migrate. Saved exams open unchanged; "file names" appears in an
  exam's spend line only once a summary has been made for it.
- File names no longer carry the exam id; two downloads of the same draft in
  the same second get the same name and the browser numbers the second.

### Tiếng Việt

#### Có gì mới

- **Tên file tải về nói lên nội dung.** Mọi file DOCX, WAV và Markdown mang
  tên `<loại đề>_<năm chữ tóm tắt>_<ngày-tháng-năm_giờ-phút-giây>`, ví dụ
  `IELTS-Listening-Part2_Riverside-Community-Garden-Volunteer-Orientation_06-10-2026_16-18-29.docx`
  (trang Whole exam bỏ phần Part:
  `HSG-Quoc-gia-Listening_Urban-Environment-And-City-Life_06-10-2026_16-25-18.docx`).
  Năm chữ do `gemini-3.5-flash-lite` viết ngay sau khi có script, từ chủ đề
  chung của đề và chủ đề các part đã có script; mỗi bản nháp tốn khoảng
  0,00005 USD, hiện ở mục "file names" trong dòng chi phí của đề. Thời gian
  lấy theo đồng hồ của trình duyệt, ngày đứng trước. DOCX và WAV của cùng một
  bản nháp có chung tên; viết lại câu hỏi, ghi âm lại hay New take thì có thời
  gian mới, còn bản ghi đầu tiên giữ tên của bộ câu hỏi. Chữ tiếng Việt được
  bỏ dấu (`Hà Nội` thành `Ha-Noi`), tên hợp lệ trên Windows, macOS và Linux.
  Nếu sau 10 giây Gemini chưa trả lời, file được đặt tên theo chính các chủ đề.
- **Tự động tải DOCX và WAV.** Ô **Download the DOCX and WAV automatically**
  trên cả hai trang, mặc định tắt, trình duyệt nhớ lựa chọn (mọi tab theo
  trong vòng 2 giây). Bật lên thì DOCX được tải ngay khi viết xong khối câu
  hỏi cuối cùng (của part, hoặc của part cuối cùng trong đề, nên mỗi đề đúng
  một lần), WAV được tải ngay khi bản ghi xong, kể cả sau "Check again". Có thể
  bật giữa chừng. Mở một đề đã lưu không bao giờ tự tải gì, kể cả bản ghi xong
  sau khi mở. Chrome và Edge hỏi một lần có cho trang tải nhiều file không:
  chọn Cho phép.
- Biến cấu hình mới `GEMINI_SUMMARY_MODEL` (mặc định `gemini-3.5-flash-lite`).
  Yêu cầu tóm tắt không gửi `thinking_level`, nên model nghĩ ở mức mặc định
  của nó (minimal với Flash-Lite, mức mà 3.8 Flash không nhận).

#### 🧰 Bên trong

- `export/naming.rs` (thuần: thời gian, bỏ dấu, loại đề, chữ tóm tắt và chữ dự
  phòng, phần tên chung, tên file), `ui/naming.rs` (mỗi trang một
  `DraftNaming`: bản tóm tắt đi kèm chủ đề nó tóm tắt, phần tên chung cố định
  từ lần tải đầu của bản nháp, mã bản nháp để lượt tải muộn không đặt tên cho
  bản nháp mới hơn, mỗi file chỉ tải một lần khi đang chờ tên),
  `ui/prefs.rs` (`localStorage`), `components/auto_download.rs`.
- Phía server: `application::naming::summarize_topics`,
  `GeminiClient::generate_brief` (model chỉ định theo từng lần gọi, không
  thinking level, 20 giây, 1.024 token ra, cùng cơ chế thử lại và đồng hồ chi
  phí), `prompts/summary.rs` (chủ đề được trích như dữ liệu, có giới hạn),
  `UsageStep::Naming`, `Bucket::Naming` (30 lần mỗi phút), giá cho 3.5
  Flash-Lite (0,30 / 0,03 / 2,50 USD mỗi triệu token, không đổi sau 2027),
  test chạy tay `brief_live_probe`.
- Tải về: đường link tạm được gắn vào trang lúc bấm, blob URL được thu hồi sau
  một phút thay vì ngay lập tức; WAV vẫn đi thẳng từ `/audio/{job_id}` xuống
  đĩa. Link WAV của trình phát thành nút bấm để đặt tên trước khi tải.

#### 🕵️ Hậu trường

- Lần chạy thật đầu tiên trên trang part dừng vì lỗi mượn (borrow panic): một
  `if let` giữ lượt đọc trạng thái trang trong khi yêu cầu tóm tắt ghi vào đó.
  Đã sửa trước khi phát hành và ghi vào `CLAUDE.md`.
- Một vòng review tìm ra: bản ghi đầu tiên tạo tên mới (nên DOCX và WAV làm
  từng bước bị lệch tên), bản tóm tắt đến muộn của đề cũ có thể đẩy bản đang
  dùng ra, lần tải đầu sau khi mở đề phải chờ tóm tắt, và dòng chi phí của đề
  không cập nhật sau khi tóm tắt. Cả bốn đã sửa và thử lại trên trình duyệt.
- Chưa sửa ở bản này: khối câu hỏi dạng điền tóm tắt (summary completion) đôi
  khi trả về `"stem": null` cho một câu, khối đó không đọc được ("Question
  generation failed: … invalid type: null, expected a string"). Gặp một lần ở
  Part 4 HSG; bấm "Regenerate questions" là được.

#### ⚠️ Cần biết khi nâng cấp

- Không cần chuyển đổi gì. Đề đã lưu mở như cũ; mục "file names" chỉ hiện
  trong dòng chi phí của đề khi đề đó đã có bản tóm tắt.
- Tên file không còn chứa mã đề; hai lần tải cùng một file của cùng bản nháp
  trong cùng một giây có cùng tên, trình duyệt tự đánh số file thứ hai.

### 📊 Con số biết nói · Numbers

| Kiểm tra · Check | Kết quả · Result |
|---|---|
| `cargo fmt --check` app và launcher · app and launcher | sạch · clean |
| `cargo check` web / server / wasm32 | sạch, **0 warning** cả ba · clean, **0 warnings** on all three |
| `cargo test --features server --no-default-features` | **194/194**, 4 test chạy tay · 4 manual (0.8.0: 181; +13: tên file, bỏ dấu, tem thời gian, giá Flash-Lite, prompt tóm tắt, yêu cầu không thinking level · names, accents, time stamp, Flash-Lite price, summary prompt, request without thinking level) |
| `brief_live_probe` (key thật · real key) | `gemini-3.5-flash-lite`: 1,2 s, 120 token vào · in, 5 ra · out, 0 thinking, ≈ 0,00005 USD → "Cities Hotel And Science Museum" |
| Trang part, IELTS Part 2, bật tự tải · Part page, IELTS Part 2, auto on | DOCX tự tải khi xong câu hỏi, WAV 9,4 MB tự tải khi xong bản ghi, **cùng tên** · DOCX on questions, 9.4 MB WAV on recording, **same name**; 5 nút tải tay dùng lại tên đó · 5 manual buttons reuse it; **1** yêu cầu Flash-Lite · Flash-Lite request (113 vào · in, 6 ra · out, 0,8 s) |
| Tắt tự tải, viết lại câu hỏi · Auto off, new questions | 0 file tự tải · auto downloads; DOCX tải tay có thời gian mới, giữ năm chữ, không gọi Gemini thêm · manual DOCX gets a new time, same words, no new call |
| Từng bước: Script only → câu hỏi → ghi âm · Step by step: script → questions → recording | DOCX và WAV **cùng tên** · **same name** (sau khi sửa · after the fix) |
| Trang Whole exam, HSG, 4 part, bật tự tải · Exam page, HSG, 4 parts, auto on | Part 4 lỗi câu hỏi → **0** file; "Regenerate questions" xong → **đúng 1** DOCX · Part 4 questions failed → **0** files; once regenerated → **exactly 1** DOCX |
| Mở đề đã lưu, bật tự tải · Opening a saved exam, auto on | **0** file tự tải · auto downloads; tóm tắt hỏi sẵn nên DOCX tải tay ra trong < 150 ms · summary fetched on open, manual DOCX in < 150 ms |
| Bản Windows 0.8.1 · Windows build | `.exe` **9,4 MB** (9.394.688 byte, 0.8.0: 9.335.808); chỉ gọi DLL hệ thống của Windows, như 0.8.0 · Windows system DLLs only, as in 0.8.0 |
| `smoke.py` trên chính file `.exe` phát hành · on the release EXE | ✅ đạt · passed; "không có key ở đâu" vẫn **không chạy được** trên máy build (key nằm trong môi trường Windows) · "no key anywhere" still **not run** on the build host (the key is in the Windows environment). Tên file trên chính `.exe` đó · Names on that EXE: `HSG-Quoc-gia-Listening-Part1_Hanoi-Weekend-Trip-Student-Discussion_06-10-2026_16-34-16_script.txt`, 0,0039 USD |
| Bản Linux 0.8.1 · Linux build | `.AppImage` **13,2 MB** (13.224.440 byte, 0.8.0: 13.183.480), build trong container Ubuntu 22.04 · built in an Ubuntu 22.04 container; fmt, check và · and 194 test chạy lại trong đó · re-run inside |
| `smoke.py` trên AppImage, Ubuntu 22.04 và 24.04 sạch · on the AppImage, clean Ubuntu 22.04 and 24.04 | ✅ đạt cả hai · passed on both, user `nobody`, kể cả · including **khởi động khi không có key ở đâu · starting with no key anywhere** |
| WASM release (chưa qua wasm-opt, vẫn crash trên Windows · not wasm-opt'd, it still crashes on Windows) | **4,21 MB** (0.8.0: 4,17 MB) |
| Tiền Gemini tốn cho 0.8.1 · Gemini spend for 0.8.1 | **khoảng 0,19 USD · about $0.19**: thử đầu-cuối trên server dev 0,186 USD (ba bản ghi ngắn là phần lớn · three short recordings are most of it), `.exe` phát hành 0,0039 USD, probe 0,00005 USD; bảy lần tóm tắt Flash-Lite tổng cộng 0,0004 USD · seven Flash-Lite summaries $0.0004 in all (`docs/portable-verification-v0.8.1.md`) |

## [0.8.0] – 2026-10-05 — 🎙️ "Mỗi người một giọng, đúng giọng vùng miền."

Ở 0.7.1, giáo viên đặt ba người nói của Part 1 HSG là *Female, British
English* và nhận về một bản ghi mà cả ba vai do **một giọng nữ Mỹ** đọc; vài
lượt giữa hội thoại còn bật thành giọng đàn ông trầm. Ba nguyên nhân, đã đo
lại trên chính file đó: giọng vùng miền chưa bao giờ tới model (30 giọng cổ
điển mà app dùng đều là General American), mỗi cặp giới + giọng vùng chỉ có
đúng một giọng (`Zephyr` cho cả A, B, C), và hai người nói chung một giọng
trong một request khiến Gemini tự "ứng biến" người thứ hai. **Từ 0.8.0, mỗi
người nói có một giọng riêng, đúng giới, đúng vùng, lấy từ thư viện giọng
vùng miền của Google; hai người trong một part không bao giờ chung giọng; và
giáo viên nghe thử, đổi, hay tự thiết kế giọng ngay trên cả hai trang.** Đọc
lại đúng đoạn HSG đó: 3 giọng thay vì 1, "tai AI" nghe ra giọng Anh thay vì
Mỹ, 0 lượt bị đổi thành giọng nam (0.7.1: 2/10). Script giờ có thể thở dài,
bật cười, ho đúng chỗ mà không một ký hiệu nào lọt vào đề hay transcript.

### Thay đổi

- **Mỗi người nói một giọng vùng miền của riêng mình.** Giọng vùng giờ nằm
  trong chính giọng đọc: app dùng giọng của Extended Voice Library
  (`en-gb-advisor-1`, `en-au-tutor-3`, `en-ie-concierge-5`…), mỗi ô giới ×
  giọng vùng có vài giọng (tiếng Anh–Anh 4–5 giọng mỗi giới, vì Part 1 HSG có
  ba người cùng giới). App tự gán: người nói trong một part không bao giờ
  chung giọng, các part của một đề ưu tiên giọng khác nhau, giọng giáo viên đã
  chọn thì giữ nguyên. Thẻ người nói hiện *Voice: Digital Assistant 1 ·
  automatic* cùng ba nút: **Listen** (nghe thử; lần đầu mỗi giọng ghi một mẫu
  ngắn, khoảng 0,005 USD, các lần sau miễn phí), **Another voice** (giọng khác
  cùng giới, cùng vùng mà chưa ai trong part dùng) và **Automatic** (để app
  chọn lại). Còn người nói chưa có giọng, hai người chung giọng hay giọng sai
  giới thì app không cho bắt đầu ghi âm.
- **Chín giọng vùng thay vì năm: thêm Irish, Scottish, South African và Indian
  English.** Bộ giọng được tuyển bằng cách cho từng giọng đọc một câu nặng
  giọng vùng, rồi đo cao độ (F0) và cho Gemini nghe lại ("tai AI"). Tai AI xác
  nhận giọng Anh, Mỹ, Scotland, Ấn Độ, Úc (nữ) và New Zealand (nam). Một số
  giọng **chỉ dựa trên danh mục của Google**, vì tai AI nghe hầu hết giọng
  không uốn "r" thành "England": toàn bộ giọng Canada và Nam Phi, một giọng
  nam Úc, hai giọng nữ New Zealand và một giọng Ireland mỗi giới. Các giọng
  này ở lại cho tới khi PO nghe mù và quyết định.
- **Sửa giọng ngay trên trang Whole exam.** Mỗi part có mục *Voices: A: …, B:
  …* với đúng thẻ người nói và hộp thoại của trang từng phần. Sửa một part thì
  app gán lại để các part vẫn khác giọng: đề IELTS thử nghiệm dùng 7 giọng
  khác nhau cho 4 part và người đọc lời dẫn.
- **Biết khi nào script hay bản ghi đã cũ.** Đổi giới, giọng vùng hay vai của
  một người nói sau khi đã có script thì trang báo tên, đại từ và cách nói có
  thể không còn khớp, và hỏi *Rewrite script and questions* hay *Keep this
  script*. Đổi giọng sau khi đã ghi âm thì trang báo *A voice changed after
  this recording was made; render it again.* App không lưu cờ nào: nó so
  người nói hiện tại với người nói lúc viết script và lúc ghi âm, nên mở một
  đề 0.7 hay để app tự gán giọng không đánh dấu gì là cũ.
- **New take: đọc lại một part, bỏ qua cache.** Gemini bỏ qua `seed`, nên
  trước đây một lượt đọc hỏng sẽ quay lại suốt 72 giờ cache. *New take of this
  part* (trang đề) và *New take (skip cache)* (trang từng phần) đọc lại đúng
  part đó và chỉ trả tiền part đó; các part khác và lời dẫn vẫn dùng lại.
- **Script biểu cảm: thở dài, bật cười, ho.** Ô *Expressive delivery (sighs,
  laughs)*, bật sẵn và lưu cùng đề, cho phép script dùng thưa thớt `<sigh>`,
  `<cough>`, `<laugh>`, `<chuckle>` giữa câu; giọng đọc diễn chúng chứ không
  đọc thành chữ. Đề, transcript, DOCX, Markdown, câu hỏi và nút *Download
  script* không bao giờ chứa các thẻ này; chỉ ô script trên màn hình giữ
  chúng cho giáo viên xem. Dù bật hay tắt, script nào cũng được yêu cầu tên,
  đại từ, chính tả và từ vựng khớp với giới và vùng của từng người nói.
- **Voice Design: tạo giọng từ một câu mô tả.** Trong hộp thoại người nói, mục
  *Designed voices* liệt kê các giọng thiết kế trong project Google của API
  key (nghe thử miễn phí, nút *Use*) và tạo giọng mới từ tên cùng một mô tả
  20–500 ký tự, ví dụ *"A warm woman in her forties with a clear Irish
  accent"*: mất khoảng 20 giây và khoảng 0,01 USD, xong thì phát mẫu và gán
  luôn cho người nói. App chỉ xoá được giọng do chính nó tạo, và chỉ tạo hay
  xoá khi chạy trên 127.0.0.1 hoặc ::1. Mỗi lượt của giọng thiết kế được đọc
  trong một request riêng.
- **Chi phí giọng nằm trong dòng chi phí của đề.** Mẫu nghe thử và giọng thiết
  kế được ghi sổ thành mục *voices* (ví dụ *voices $0.011*) và dòng chi phí
  cập nhật ngay khi mẫu hay giọng mới về.
- **Ô chọn hiện đúng giá trị đã lưu.** Trước đây hộp thoại người nói luôn hiện
  *British English* và *Student* dù đã lưu gì, không chọn lại được mục đầu
  tiên, và ô Format quay về IELTS sau khi tải lại trang.

### 🧰 Bên trong

- `domain/voice.rs`: `Voice`, `VoiceSource` (Library | Designed),
  `VoiceChoice` (Auto | Assigned | Chosen), `assign_voices` /
  `assign_exam_voices` (giữ giọng giáo viên chọn, giữ giọng đã gán nếu còn
  hợp, người còn lại lấy giọng trống đầu tiên của pool, ưu tiên giọng part
  khác chưa dùng; tất định, không bao giờ trùng), `next_voice`,
  `voice_conflicts`, `speaker_change`, `VoiceDesignRequest`. Cùng một luật
  chạy trên trình duyệt và trên server (`prepare_speakers`, gán lại lúc bắt
  đầu job rồi trả danh sách người nói đã dùng về cho trang). `describe()`
  không bao giờ nêu tên giọng, vì tên đó sẽ lọt vào script.
  `Passage.written_for` và `ExamPart.recorded_for` là cơ sở để suy ra "cũ".
- Danh mục giọng: `tts/default_voices.json` biên dịch vào app (pool theo giọng
  vùng × giới, sắp theo độ khác nhau, announcer `en-gb-tutor-9`);
  `voices.json` version 2 chỉ ghi đè (một danh sách không rỗng thay một pool);
  pool dưới ngưỡng (Anh 4, các giọng lõi 3, giọng mới 2) là một dòng thông báo
  khi khởi động.
- `plan_passage`: một bảng giọng cho cả passage; đoạn tối đa 200 từ và 2 giọng,
  chỉ gộp lượt cùng người nói và cùng style; giọng thiết kế đọc riêng từng
  lượt; nối 250 ms khi đổi người, 350 ms khi cùng người. Style của mỗi lượt là
  một câu ngắn cố định theo vai (`SpeakerRole::delivery_style`, ví dụ *polite
  and helpful*) cộng `EXAM_PACE`; không bao giờ có giọng vùng, giới, tuổi hay
  tên trong style.
- Cache giọng đọc v2 (`speech-cache-v2`): khoá gồm model, (nhãn, id giọng) và
  (người nói, style, lời) của từng lượt. `Reuse::Refresh` cho *New take*: đọc
  lại và ghi đè đoạn cũ.
- `domain/speech.rs` là ngữ pháp duy nhất của thẻ giọng: `SPEECH_TAGS` (theo
  tài liệu Google), `EXAM_SPEECH_TAGS` (4 thẻ đã đo), `READ_ALOUD_TAGS`,
  `display_text` cho transcript, đếm từ và kiểm grounding, `speech_text` cho
  TTS (bỏ thẻ lạ, `[ghi chú]`, dấu `|` lạc vì sẽ bị đọc ra),
  `markup_problems` thành cảnh báo nêu đúng lượt.
- `GeminiClient`: mọi request đi qua một đường `send` (retry, header, theo dõi
  key bị từ chối). Thêm Voices API: `list_voices`, `get_voice`,
  `delete_voice`, `create_voice` (`POST /v1beta/voices`, `"store": true`,
  request lưu trữ duy nhất của app, không bao giờ thử lại sau timeout, tính
  phí ước tính theo giá TTS vì Google chưa công bố giá). Tối đa **3** request
  TTS cùng lúc trong cả process; 429 đọc `retryDelay` của Google (tối đa 60
  s). Lỗi về giọng (`UnknownVoice`, `VoiceLimit`) không bao giờ bị coi là key
  hỏng.
- `tts/samples.rs` và route thường `GET /voice-sample/{voice_id}`: mẫu giọng
  nằm ở `DATA_DIR/audio/voices`, dọn sau 30 ngày không dùng.
  `Bucket::VoiceSample` 30 lần/phút, `Bucket::VoiceDesign` 5 lần/phút.
- Bảng `designed_voices` trong `jobs.db`: những giọng app đã tạo, giữ đúng
  giọng vùng (Google chỉ giữ mã ngôn ngữ, Scotland và Anh đều là `en-GB`).
  Danh sách giọng của project được nhớ 60 giây.
- `UsageStep::Voices` và `ExamUsage.voices`; request không tốn tiền thì không
  ghi hàng nào.
- `tools/voice_lab.py` (stdlib + numpy, không đóng gói): `catalog`, `probe`,
  `audition`, `score`, `report`, có trần chi tiêu, đọc key giống app và chỉ
  gửi trong header. F0 đo bằng YIN; "tai AI" là `gemini-3.8-flash` nghe audio
  và trả JSON giới, giọng vùng, số người nói, chữ bị đọc ra. `docs/voices.md`
  ghi nguyên nhân, 9 cổng quyết định (G1–G9) và cách chọn pool. Test chạy tay
  mới: `voice_live_probe`.
- Một đề 0.7.1 thật (`fixtures/saved_exam_0_7_1.json`) nằm trong unit test:
  vẫn mở được, không part nào bị đánh dấu cũ.

### 🕵️ Hậu trường: những điều chỉ đo mới biết

- **Giọng nam "ứng biến" là do trùng giọng.** Với đúng request của 0.7.1,
  `Zephyr` + `Zephyr` biến người thứ hai thành giọng nam trong 3/5 lần đọc;
  hai giọng `en-gb` khác nhau: 0/5. Vì thế "không bao giờ trùng giọng" là luật
  của domain, không phải tuỳ chọn.
- **`<long pause>` và `<whispers>` bị đọc thành chữ** ("Long pause.",
  "Whispers, don't tell anyone.") dù Google liệt kê chúng. Chỉ 4 thẻ không
  bao giờ bị đọc ra và được diễn trong đa số lần thử mới được dùng.
- **Style đổi theo từng dòng làm giọng trôi**: một dòng "calm" hạ cao độ 18 %,
  một lượt "worried" nâng 20 %. Cảm xúc từng dòng vì thế chỉ đi qua lời, dấu
  câu và 4 thẻ trên; style cố định theo người nói.
- **Google nhận giọng thiết kế trong request hai giọng**, trái với tài liệu.
  App vẫn cho giọng thiết kế đọc riêng như tài liệu nói; một hằng số đổi được
  khi Google xác nhận.
- **Mỗi giọng trong request tính thêm audio tham chiếu của nó** thành token
  đầu vào: giọng cổ điển 200–260, giọng thư viện 740–1.970, giọng thiết kế
  490–1.220.
- **Tai AI không phân biệt được các giọng không uốn "r"**: nó nghe giọng Nam
  Phi 6/6, Ireland 4/6, New Zealand 3/8 và Úc 2/8 thành "England", và từng
  gọi một giọng nữ Anh 188 Hz với một giọng nữ Úc 149 Hz là cùng một người.
  Nó chỉ được dùng để lọc, còn quyết định là của người nghe.
- **Kiểm thử đầu-cuối tóm một lỗi nhỏ**: sau khi bấm *Listen* (tốn tiền) hay
  tạo giọng, dòng chi phí của đề đứng yên tới bước kế tiếp. Đã sửa trước khi
  phát hành.

### ⚠️ Cần biết khi nâng cấp

- **Lần render lại đầu tiên của đề cũ phải trả tiền một lần.** Khoá cache đổi
  sang v2 (giọng và style mới), nên không đoạn nào của 0.7 được dùng lại:
  khoảng 0,32 USD cho bản ghi của một đề IELTS. Từ lần sau, render lại không
  đổi gì vẫn là 0 đồng.
- **`voices.json` của 0.7 không còn được dùng.** Nếu file vẫn đúng mặc định
  của 0.7, app đổi tên nó thành `voices.0.7.json` và ghi template version 2.
  Nếu đã sửa tay, app giữ nguyên từng byte nhưng bỏ qua, và console báo
  *0.7 format*: muốn ghi đè giọng thì viết lại theo version 2, hoặc xoá file
  để lấy template. File 0.7 hỏng vẫn chặn khởi động như trước.
- **Đề 0.7 nhận giọng tự động mới khi mở.** Không gì bị đánh dấu cũ và bản ghi
  cũ vẫn phát được, nhưng render lại sẽ dùng giọng vùng miền mới.
- **Giọng thiết kế gắn với project Google của API key.** Đổi sang key của
  project khác thì giọng đó không dùng được (app báo rõ khi bắt đầu ghi âm).
  Chỉ tạo và xoá được khi app nghe trên 127.0.0.1 hoặc ::1, nên không làm được
  trong Docker hay trên server mở cho máy khác. Tối đa 200 giọng mỗi project;
  Google giữ một giọng một năm kể từ lần dùng cuối.
- **Đề lưu ở 0.8 đừng mở lại bằng 0.7.** Đề có người nói Irish, Scottish,
  South African hay Indian không mở được ở 0.7. Đề khác mở được, nhưng 0.7 bỏ
  mất giọng đã chọn và dấu "viết cho / ghi cho", in thẻ `<sigh>` ra
  transcript, và lần tự lưu kế tiếp ghi đè những thứ đó.
- **Expressive bật sẵn.** Muốn script như 0.7, không thẻ nào, thì bỏ chọn
  *Expressive delivery* trước khi sinh script.

### 📊 Con số biết nói

| Kiểm tra | Kết quả |
|---|---|
| `cargo fmt --check` app và launcher | sạch |
| `cargo check` web / server / wasm32 | sạch, **0 warning** cả ba |
| `cargo test --features server --no-default-features` | **181/181**, 3 test chạy tay (0.7.1: 86; thêm 95 test: gán giọng, danh mục và `voices.json` v2, chia đoạn theo giọng, cache v2, thẻ giọng, transcript không thẻ, Voices API và Voice Design, đề 0.7.1 vẫn mở, suy ra "cũ") |
| Bản Windows 0.8.0 | `.exe` **9,3 MB** (9.335.808 byte, 0.7.1: 8.733.696); chỉ gọi DLL hệ thống của Windows, như 0.7.1 |
| `smoke.py` trên chính file `.exe` phát hành | ✅ đạt, kể cả `voices.json` version 2 lần đầu chạy, file 0.7 mặc định bị đổi tên, file 0.7 đã sửa được giữ nguyên, và đóng cửa sổ console; "không có key ở đâu" vẫn **không chạy được** trên máy build (key nằm trong môi trường Windows) |
| Giọng trên chính file `.exe` phát hành, thư mục cấu hình mới | 3 thẻ người nói có giọng tự động (*Digital Assistant 1*, *Sola*, *Authoritative Advisor 8*); *Listen* ghi một mẫu 17 giây, 1 request, **0,0054 USD**, phát ngay trong trang; nghe lần hai không gọi Google |
| Bản Linux 0.8.0 | `.AppImage` **13,2 MB** (13.183.480 byte, 0.7.1: 12.634.616), build trong container Ubuntu 22.04; fmt, check và 181 test chạy lại trong đó |
| `smoke.py` trên AppImage thật, Ubuntu 22.04 và 24.04 sạch | ✅ đạt cả hai, user `nobody`, kể cả `voices.json` 0.7 và **khởi động khi không có key ở đâu** |
| WASM release (chưa qua wasm-opt, vẫn crash trên Windows) | **4,17 MB** (0.7.1: 3,88 MB) |
| HSG Part 1, cùng đoạn 701 từ, ba người *Female, British*: số giọng gửi đi | 0.7.1: 1 (`Zephyr` cho A, B, C) → 0.8.0: **3** giọng `en-gb` khác nhau |
| Cùng đoạn đó, tai AI nghe | nữ, **Mỹ** → nữ, **Anh** |
| Cùng đoạn đó, lượt bị đổi thành giọng nam (F0 dưới 120 Hz) | **2/10 → 0** |
| Cùng đoạn đó, request / tiền ghi âm | 6 / 0,093 USD → 6 / 0,106 USD |
| Part 1 HSG mới sinh trong app (666 từ, Expressive bật, ba người *Female, British*) | 3 giọng khác nhau, 6 request TTS không cái nào chung giọng, F0 0 cờ, tai AI: 3 nữ, Anh (0,95); 9 request, **0,097 USD** (giá 2027: 0,194), 62 giây từ lúc bấm tới WAV 4:39 |
| Đề IELTS đầy đủ, Part 3 giọng nữ Ireland + nam Scotland | **0,364 USD** (giá 2027: **0,728**, vượt ngân sách 0,70), 33 request, bản ghi 30:18, cả quy trình 3 phút 6 giây; 7 giọng khác nhau cho 4 part và lời dẫn |
| Trong đó bản ghi âm | 0,322 USD, 19 request (13 đoạn của các part, 6 lời dẫn; 1 lần thử lại vì Google báo bận) |
| Part 3 qua tai AI | nữ Ireland và nam Scotland (0,95); `<sigh>`, `<chuckle>` được diễn, không bị đọc ra |
| DOCX của đề | không có `&lt;`, `\|` hay `[` nào |
| Render lại đề không đổi gì | 0 request, 19/19 đoạn dùng lại, **0 USD**, 8 giây |
| *New take of this part* cho Part 1 | 3 request, 16 đoạn dùng lại, 0,052 USD (giá 2027: 0,104) |
| *Listen* lần đầu / lần sau | khoảng 0,005 USD, 8 giây / **0 USD**, 0,7 giây |
| Voice Design: tạo một giọng | khoảng 0,009 USD (ước tính theo giá TTS), 18 giây; 80 giây khi Google báo bận |
| Tiền Gemini tốn cho 0.8.0 | khoảng 1,3 USD thăm dò và tuyển giọng (`docs/voices.md`), 0,025 USD thử trong app, 0,546 USD kiểm thử đầu-cuối, 0,005 USD nghe thử trên file `.exe` phát hành: **khoảng 1,9 USD** (chi tiết trong `docs/portable-verification-v0.8.0.md`) |

### Thêm

- `src/domain/voice.rs`, `src/domain/speech.rs`, `src/application/voices.rs`,
  `src/infrastructure/tts/default_voices.json`,
  `src/infrastructure/tts/samples.rs`, `src/infrastructure/tts/designed.rs`,
  `src/ui/components/voices.rs`, `tools/voice_lab.py`, `docs/voices.md`.
- Route `GET /voice-sample/{voice_id}`; bảng `designed_voices` trong `jobs.db`.
- Phiên bản app và launcher portable: 0.8.0. Không thêm dependency nào.

### Biết rồi, để bản sau

- **Theo giá 2027, một đề IELTS vượt ngân sách**: 0,728 USD so với 0,70 (mức
  chất lượng trong `docs/project_scope.md`). Một phần vì mỗi giọng thư viện
  tính thêm audio tham chiếu trong mọi request, một phần vì bản ghi dài hơn.
  Hôm nay vẫn là 0,364 USD.
- Các giọng chỉ dựa trên danh mục (Canada, Nam Phi, một giọng nam Úc, hai giọng
  nữ New Zealand, một giọng Ireland mỗi giới) chờ PO nghe mù bảng tuyển giọng.
- Giọng nữ Ireland `en-ie-concierge-5` khá trầm (138–147 Hz): báo cáo F0 gắn
  cờ "giáp ranh", không phải cờ giọng nam; tai AI vẫn nghe là nữ.
- Một part có giọng thiết kế tốn nhiều request hơn (mỗi lượt một request) và
  Google chưa công bố giá Voice Design, nên chi phí tạo giọng là ước tính.
- Part 1 của đề IELTS kiểm thử còn 3 issue của validator, đều về câu hỏi chứ
  không về giọng: mã bưu chính *BR6 9TY* và số điện thoại *07700 900352* bị
  tính là 2 từ so với *ONE WORD AND/OR A NUMBER* (có lẽ nên coi là một số),
  và đáp án *30.50* không có nguyên văn trong script (script nói *thirty
  pounds and fifty pence*).
- `gemini-3.8-flash-lite-tts` nhận giọng thư viện, nhưng cùng một giọng nữ cao
  hơn 20–24 % so với Flash TTS; chưa ai nghe trọn một đề trên lite.
- Voice Replication (giọng làm từ bản ghi người thật) không làm trong bản này.

## [0.7.1] – 2026-10-05 — 🔑 "Key hỏng thì nói thẳng, và cho thay ngay trên trang."

Ở 0.7.0, một key đã thu hồi hay gõ sai trong biến môi trường Windows là đủ để
mọi bước báo một câu khó hiểu, trong khi ô nhập key trên trang lại khoá vì
"đã có key". **Từ 0.7.1, app nói key hỏng nằm ở đâu, và trên máy của chính
giáo viên thì cho dán key khác ngay trên trang, không cần khởi động lại.**
Không có gì thay đổi khi key vẫn chạy.

### Thay đổi

- **Key bị Google từ chối thì nói rõ, và cho thay ngay trên trang.** Trước
  đây, key sai trong biến môi trường hay `.env` hiện ra thành câu
  *The AI service rejected the request (400): API key not valid…* (lộ mã HTTP,
  không nói key nằm đâu), ô nhập key bị khoá vì "đã có key", và chỉ còn cách
  sửa rồi khởi động lại. Giờ lỗi là *Google rejected the Gemini API key from
  the Windows environment…*, đến từ mọi bước (topic, script, câu hỏi, ghi âm).
  Trong vòng 5 giây, đầu trang hiện hộp *Google rejected the Gemini API key*;
  trên app chạy ở 127.0.0.1 hoặc ::1, hộp này cho dán key khác (vẫn kiểm tra
  miễn phí với Google trước). Key mới thay key hỏng trong bộ nhớ đến lần khởi
  động lại. *Remember* chỉ hiện khi ghi vào `.env` thật sự có tác dụng: không
  hiện nếu key hỏng nằm trong biến môi trường, vì biến môi trường luôn thắng
  `.env`. Server mở cho máy khác thì vẫn không nhận key từ trình duyệt.
- Lỗi của Google bọc trong mảng JSON (`[{"error": …}]`, kiểu Interactions API
  trả cho lỗi key) giờ được đọc đúng câu thông báo thay vì in nguyên body.

### 🧰 Bên trong

- `error_of` nhận ra lỗi key: 401 luôn là lỗi key; 400 và 403 chỉ khi Google
  nói về key (`API_KEY_INVALID`, *API key not valid*, *Please use API Key*),
  vì hai mã này còn dùng cho request sai hay model bị chặn. Body thật lấy từ
  hai lần gọi thử miễn phí (key bịa, không key) nằm trong unit test.
- `GeminiClient` đổi lỗi key thành `LlmError::ActiveKeyRejected(nguồn)` và
  báo cho `secrets`, nơi giữ (chỉ trong RAM) key bị từ chối gần nhất; Google
  nhận lại key đó thì dấu tự xoá. Không thử lại, không tự chuyển sang nguồn
  key khác.
- `KeyStatus` thêm `rejected` và `can_remember`; `entry_refusal` nhận thêm
  `rejected`. `KeySetup` hỏi `api_key_status` mỗi 5 giây (server trả lời từ
  bộ nhớ, không gọi Google), nên lỗi key ở bất kỳ bước nào cũng làm hộp hiện
  ra mà không phải sửa từng chỗ báo lỗi trong các view.

### 📊 Con số biết nói

| Kiểm tra | Kết quả |
|---|---|
| `cargo fmt --check` app và launcher | sạch |
| `cargo check` web / server / wasm32 (`--locked`) | sạch, **0 warning** cả ba |
| `cargo test --features server --no-default-features` | **86/86**, 2 test chạy tay (0.7.0: 84; thêm 2 test: body lỗi key thật của Google, khi nào *Remember* có tác dụng) |
| Bản Windows 0.7.1 | `.exe` **8,7 MB** (8.733.696 byte, 0.7.0: 8.732.672); chỉ gọi DLL hệ thống của Windows, như 0.7.0 |
| `smoke.py` trên chính file `.exe` phát hành | ✅ đạt, kể cả đóng cửa sổ console; "không có key ở đâu" vẫn **không chạy được** trên máy build (key nằm trong môi trường Windows) |
| Luồng key hỏng trên chính file `.exe` phát hành, key bịa trong biến môi trường | lỗi nói *from the server's environment*; hộp hiện, ô nhập che ký tự, không có *Remember* |
| Bản Linux 0.7.1 | `.AppImage` **12,6 MB** (12.634.616 byte, 0.7.0: 12.618.232), build trong container Ubuntu 22.04; fmt, check và 86 test chạy lại trong đó |
| `smoke.py` trên AppImage thật, Ubuntu **22.04** và **24.04** sạch, user `nobody` | ✅ đạt cả hai, **kể cả khởi động khi không có key ở đâu** |
| WASM release (chưa qua wasm-opt, vẫn crash trên Windows) | **3,88 MB**, như 0.7.0 |
| Tiền Gemini tốn cho toàn bộ kiểm tra | **0 USD**: mọi request dùng key bịa hoặc không key, Google từ chối miễn phí |

### Biết rồi, để bản sau

- Các lỗi khác ngoài lỗi key vẫn hiện mã HTTP, ví dụ *rejected the request
  (400)*.
- Chưa thử thay key hỏng bằng một key thật trên trình duyệt; đã thử với key
  bịa trong biến môi trường (hộp hiện, không có *Remember*, key bịa thứ hai bị
  từ chối) và với key thật trong môi trường Windows (hộp không hiện).

## [0.7.0] – 2026-09-28 — 💸 "Một đề IELTS dưới 0,70 USD, kể cả khi Google tăng giá."

Trước bản này, không ai biết một đề tốn bao nhiêu tiền Gemini: app không đọc
con số nào Google trả về, và giọng đọc chạy trên `gemini-2.5-pro-preview-tts`,
một bản preview đã vào danh sách ngừng hỗ trợ, giá 20 USD cho mỗi triệu token
audio. **Từ 0.7.0, chữ chạy trên Gemini 3.8 Flash, giọng trên Gemini 3.8 Flash
TTS, mọi request đi qua Interactions API, và từng token, từng xu của mỗi đề
được ghi sổ.** Đo thật: một đề IELTS đầy đủ (4 topic, 4 script, 6 khối câu
hỏi, bản ghi 29 phút) tốn **0,308 USD** hôm nay và **0,616 USD** theo giá Google
công bố từ 01/01/2027, dưới mức 0,70 USD. Key API giờ lấy từ biến môi trường
Windows trước, rồi `.env`; không có thì hỏi ngay trên trang.

### ✨ Điểm nhấn

- **Biết đề này tốn bao nhiêu, ngay trên trang.** Dòng *Gemini spend for this
  exam: $0.308 of $0.700 (topics $0.001, scripts $0.012, questions $0.026,
  recording $0.269)* nằm dưới trạng thái lưu, cập nhật sau mỗi bước; di chuột
  vào để xem số request, số token vào, ra, thinking. Vượt `EXAM_BUDGET_USD`
  (mặc định 0,70) thì hiện cảnh báo, **không chặn gì**. Khung Saved exams hiện
  tổng 24 giờ và 30 ngày của cả server, gồm cả trang từng phần.
- **Tiền đã trả là được ghi, kể cả khi hỏng.** Mỗi bước chạy (topic, script,
  khối câu hỏi, job ghi âm) ghi một hàng vào sổ `usage` trong `jobs.db`, dù
  thành công, lỗi hay bị lần chạy mới thay thế. Mỗi request tính theo đúng
  biểu giá lúc gọi: giá khuyến mãi của 3.8 tới 31/12/2026, giá niêm yết (gấp
  đôi) từ 01/01/2027. Xoá đề không xoá sổ.
- **Render lại đề không đổi gì: 0 đồng, vài giây.** Mỗi đoạn giọng đọc được
  giữ lại theo dấu vân tay của (model, giọng, lời, cách đọc). Trước đây mỗi
  lần bấm *Render exam audio* là trả lại trọn khoảng 0,27 USD và chờ vài phút;
  giờ đề không đổi thì 20/20 đoạn dùng lại, sửa một phần thì chỉ trả tiền phần
  đó, lời dẫn của đề chỉ trả một lần. `SPEECH_CACHE_HOURS` (mặc định 72, `0` là
  tắt) quyết định giữ bao lâu kể từ lần dùng cuối.
- **Thinking `low` mặc định, phần chữ rẻ đi khoảng 80 %.** Ở `low`, 14 request
  chữ của một đề không tốn token thinking nào: 0,039 USD thay vì 0,19 USD ở
  `medium`. Validator vẫn soi như cũ (đề đo ở `low` có 3 lỗi giới hạn từ ở
  phần 4, đề `medium` cũng có lỗi cùng loại). Muốn nghĩ kỹ hơn thì đặt
  `GEMINI_THINKING_LEVEL=medium` hoặc `high` trong `.env`.
- **Key API: môi trường Windows → `.env` → trình duyệt.** Key đặt bằng `setx`
  hay hộp thoại *Environment Variables* được đọc thẳng từ registry (của user
  rồi của máy), nên terminal hay IDE mở từ trước vẫn thấy. Không có key ở đâu,
  app vẫn khởi động và mọi trang hiện ô dán key dưới một **cảnh báo bảo mật
  nghiêm túc**: key đi qua HTTP thường, app không có đăng nhập, *Remember* ghi
  key không mã hoá vào `.env`. Ô nhập chỉ nhận key khi app nghe trên
  127.0.0.1 hoặc ::1, không bao giờ ghi đè key của môi trường hay `.env`, và
  hỏi Google (miễn phí) xem key có dùng được không trước khi giữ. Key không
  bao giờ quay về trình duyệt hay vào log; console chỉ nói key lấy từ đâu.
- **Google không giữ bản sao nào.** Mọi request gửi kèm `"store": false`; mặc
  định Interactions API lưu mỗi lần gọi 55 ngày.

### 🧰 Bên trong

- `GeminiClient` viết lại cho `POST /v1beta/interactions`: body snake_case,
  model ghim cứng `gemini-3.8-flash` và `gemini-3.8-flash-tts`, đọc bước
  `model_output` cuối (bỏ bước `thought`). `status: incomplete` (hết
  `max_output_tokens`, giờ chặn ở 8.192), `failed` và mã chặn nội dung
  (`safety`, `recitation`…) thành câu lỗi dễ đọc. Thử lại thêm cả 504. Bỏ
  đường vòng `responseMimeType` của API cũ.
- TTS 3.8 đọc **nguyên văn**: cách đọc nằm trong `speech_metadata.style`,
  người nói trong `speech_metadata.speaker` (`SpeakerA`, tức nhãn bỏ dấu cách);
  văn bản chỉ còn đúng lời thoại. Script được cắt thành đoạn tối đa 200 từ và
  2 người nói (bài giảng dài cắt ở cuối câu), nối bằng 350 ms im lặng. Phần 1
  HSG ba giọng giờ là vài đoạn hai giọng thay vì đọc từng lượt một. App xin
  PCM thô 24 kHz; WAV trả về vẫn đọc được.
- Bộ đếm usage dùng chung giữa các bản sao của client, nên các phần chạy song
  song trong một job ghi âm cộng vào một tổng; job ghi âm ghi sổ cả khi thất
  bại. Mọi server function gọi Gemini nhận thêm `exam: Option<Uuid>`; trang
  từng phần truyền `None`.
- Test sống `live_probe` (`#[ignore]`, khoảng 0,01 USD): 1 text, 1 JSON, 2 TTS,
  in token, độ trễ và số token audio mỗi giây.

### 🕵️ Hậu trường: những bug bị tóm trước khi tới tay giáo viên

- **Câu dẫn cho giọng đọc sẽ bị đọc thành tiếng.** Tài liệu của 3.8 TTS nói
  rõ văn bản được đọc nguyên văn, nên câu cũ *"Read the following
  listening-exam script aloud…"* sẽ vang lên ở đầu mỗi phần. Đã chuyển sang
  `speech_metadata` rồi kiểm bằng cách cho Gemini chép lại audio: đúng từng
  chữ, không có câu dẫn hay nhãn người nói, hai giọng luân phiên đúng.
- **`"shared_options": null` làm hỏng 2/6 khối câu hỏi.** Lần đo đầu, 3.8 Flash
  trả `null` thay vì bỏ trống danh sách lựa chọn ở câu trắc nghiệm phần 2 và
  3; hai khối đó đã tính tiền mà không dùng được. Giờ `null` trong
  `shared_options`, `options` và `evidence` được đọc như rỗng, có test giữ
  đúng dạng payload đã gặp.
- **Âm thanh tính 32 token mỗi giây, không phải 25 như trang giá.** Dự toán
  ban đầu thấp hơn thực tế 28 %. Probe đo được 32,0 và 32,1; ngân sách, bảng
  chi phí và phần dự phòng khi thiếu số liệu đều tính lại theo số đo.
- **Mỗi đoạn 250 từ mất khoảng 40 giây**, sát ngưỡng một phút mà request thường
  bị đóng. Đoạn giờ tối đa 200 từ, khoảng 30 giây.
- **Ở `medium`, một đề sẽ vượt ngân sách từ 2027** (0,961 USD). Đó là lý do
  `low` là mặc định.

### 📊 Con số biết nói

| Kiểm tra | Kết quả |
|---|---|
| `cargo fmt --check` app và launcher | sạch |
| `cargo check` web / server / wasm32 | sạch, **0 warning** cả ba |
| `cargo test --features server --no-default-features` | **84/84**, 2 test chạy tay (0.6.0: 53; thêm 31 test: thứ tự key, ghi `.env`, chặn key từ trình duyệt, body Interactions, đọc usage và giá, chia đoạn, cache, sổ usage, `null` options) |
| Bản Windows 0.7.0 | `.exe` **8,7 MB** (0.6.0: 8,5 MB); chỉ gọi DLL hệ thống của Windows |
| `smoke.py` trên chính file `.exe` phát hành | ✅ đạt, kể cả đóng cửa sổ console; trường hợp "không có key ở đâu" **không chạy được** trên máy build (xem dưới) |
| Bản Linux 0.7.0 | `.AppImage` **12,6 MB** (0.6.0: 12,5 MB), build trong container Ubuntu 22.04; fmt, check và 84 test chạy lại trong đó |
| `smoke.py` trên AppImage thật, Ubuntu **22.04** và **24.04** sạch, user `nobody` | ✅ đạt, **kể cả khởi động khi không có key ở đâu**: server chạy, `/exam` trả 200 |
| WASM release (chưa qua wasm-opt, vẫn crash trên Windows như 0.5.0) | 3,79 MB → **3,88 MB** |
| Probe sống | cả 4 dạng request được chấp nhận; audio **32 token/giây**; TTS nhanh khoảng 2,5 lần thời gian thực |
| Đề IELTS đầy đủ, thinking `medium` | 0,480 USD (giá 2027: 0,961, vượt ngân sách) |
| Đề IELTS đầy đủ, thinking `low` | **0,308 USD** (giá 2027: **0,616**), 34 request, bản ghi 29:10 |
| Trong đó bản ghi âm | 0,269 USD cho 29.728 token audio, gần 90 % chi phí |
| Render lại đề không đổi gì | 0 request, 20/20 đoạn dùng lại, **0 USD** |
| Server không có key trong process nhưng có trong registry | console báo *from the Windows environment* |
| Tiền Gemini tốn cho toàn bộ kiểm tra | khoảng **0,80 USD** |

### Thêm

- `src/domain/usage.rs`, `src/application/settings.rs`,
  `src/application/usage.rs`, `src/infrastructure/secrets.rs`,
  `src/infrastructure/usage.rs`, `src/infrastructure/llm/pricing.rs`,
  `src/infrastructure/tts/cache.rs`, `src/ui/components/key_setup.rs`,
  `docs/portable-verification-v0.7.0.md`.
- Biến `GEMINI_THINKING_LEVEL`, `EXAM_BUDGET_USD`, `SPEECH_CACHE_HOURS`
  (`.env.example`, template portable, README).
- Dependency `sha2` 0.10 (server) và `winreg` 0.55 (chỉ Windows).

### Thay đổi

- Mặc định `GEMINI_TEXT_MODEL=gemini-3.8-flash` (thay alias
  `gemini-flash-latest`), `GEMINI_TTS_MODEL=gemini-3.8-flash-tts`.
- Rate limit riêng cho việc nhập key (5 lần/phút).
- `packaging/smoke.py` và `packaging/linux/check-clean.sh` kiểm tra cách khởi
  động mới khi thiếu key.
- Phiên bản app và launcher portable: 0.7.0.

### ⚠️ Thay đổi không tương thích

- **Model TTS cũ không còn chạy.** Request giờ theo định dạng của TTS thế hệ
  3.8; ai đã đặt `GEMINI_TTS_MODEL=gemini-2.5-…` hay `gemini-3.1-…` trong `.env`
  phải xoá dòng đó hoặc đổi sang `gemini-3.8-flash-tts` /
  `gemini-3.8-flash-lite-tts`.
- **Thiếu key không còn làm app thoát.** App khởi động và hỏi trên trang; script
  nào trông vào "thoát mã 1 khi thiếu key" phải sửa (smoke test đã sửa).
- **Biến môi trường Windows thắng `.env`.** Máy có sẵn một `GEMINI_API_KEY` cũ
  trong môi trường Windows sẽ dùng key đó thay cho key trong `.env`; console
  nói rõ key lấy từ đâu.
- Model nằm ngoài bảng giá (ví dụ một alias) vẫn chạy, nhưng request của nó ghi
  là *chưa có giá* và số tiền hiện dạng `$0.012+?`.

### Biết rồi, để bản sau

- **Ô nhập key chưa được ai nhìn thấy trong trình duyệt.** Trên Windows, máy
  build có `GEMINI_API_KEY` trong môi trường nên `smoke.py` in *NOT TESTED*;
  trên Linux smoke test xác nhận server khởi động không cần key, nhưng không
  mở trình duyệt. Các luật chặn có unit test.
- Chưa test mount FUSE thật của AppImage (container không có `/dev/fuse`).
- Giá 2027 chỉ còn dư khoảng 0,08 USD mỗi đề và bản ghi âm chiếm gần 90 % chi
  phí. `gemini-3.8-flash-lite-tts` rẻ hơn một phần ba nhưng chưa ai nghe thử.
- Giá nằm cứng trong `src/infrastructure/llm/pricing.rs`: Google đổi giá thì
  phải sửa code.
- Cache giọng đọc chỉ dọn theo thời gian, không giới hạn dung lượng: một đề
  IELTS khoảng 45 MB.
- Chưa ai nghe chỗ nối giữa các đoạn trong một bản ghi đầy đủ.
- Trang từng phần chưa có dòng chi phí riêng; chi phí của nó chỉ hiện trong
  tổng của cả server.

## [0.6.0] – 2026-09-23 — 💾 "Đóng tab, mai mở lại vẫn còn."

Ba việc giáo viên hay vấp nhất sau khi có bản portable: đóng trình duyệt là mất
đề, bản ghi âm biến mất sau một ngày, và chỉ xuất được Markdown. **Từ 0.6.0,
đề ở trang "Whole exam" được lưu ngay trên máy chạy app, bản ghi âm của đề đã
lưu không bao giờ bị dọn, và cả hai trang xuất được file Word đúng dáng giấy
thi.** Không cần cài thêm gì; chép cả thư mục portable sang máy khác thì đề và
audio đi theo.

### ✨ Điểm nhấn

- **Đề tự lưu, không cần nhớ bấm.** Có script đầu tiên là app bắt đầu tự lưu:
  sau mỗi bước sinh xong (script, câu hỏi, ghi âm), một giây sau khi sửa tiêu
  đề, chủ đề hay topic, và khi bấm nút **Save**. Dòng trạng thái nói rõ
  *Saving... / Saved at 18:37 / Unsaved changes / Save failed*. Lưu chạy đơn
  luồng: đang lưu mà có thay đổi mới thì xếp hàng, xong là lưu tiếp, không bao
  giờ ghi chồng bản cũ lên bản mới.
- **Danh sách "Saved exams" ngay trên trang.** Tiêu đề, định dạng, tiến độ
  (script x/4, câu hỏi y/4), trạng thái ghi âm, giờ cập nhật. **Open** khôi
  phục đúng những gì đã thấy: topic từng phần, issue của validator (tính lại
  bằng đúng bộ kiểm tra thuần, không lưu), và bản ghi âm. Nếu job ghi âm còn
  đang chạy lúc đóng tab, mở lại là app poll tiếp và hiện player khi xong.
  **Delete** hỏi lại lần hai; xoá đề đang mở thì trang về đề mới, không có
  hàng nào "sống lại" vì auto-save.
- **Audio của đề đã lưu giữ mãi.** Job ghi âm mà một đề đã lưu trỏ tới bị bỏ
  qua khi dọn dẹp, chỉ xoá khi xoá đề (và chỉ khi không đề nào khác dùng
  chung). Audio lẻ (trang từng phần, đề chưa lưu) dọn theo biến mới
  `AUDIO_RETENTION_HOURS`: mặc định 24, đặt `0` là không bao giờ dọn (task dọn
  không được tạo). Giá trị sai bị chặn ngay lúc khởi động, nêu đúng tên biến.
- **Chuyển thư mục portable không mất audio.** Trước đây hàng job lưu đường
  dẫn tuyệt đối của WAV, nên đổi ổ đĩa hay chép sang máy khác là `/audio/…`
  trả 404. Giờ hàng job chỉ lưu tên file; mọi nơi đọc đều ghép với `DATA_DIR`
  hiện tại. Hàng cũ ghi đường dẫn tuyệt đối (Windows lẫn Linux) vẫn đọc được.
- **Xuất Word (DOCX) ở cả hai trang.** Bố cục theo đề HSG: tiêu đề, dòng
  định dạng, khối *Họ và tên / Số báo danh / Số phách / Điểm* để trống, mỗi
  task có rubric in đậm, lựa chọn, câu hỏi, rồi **bảng ô đáp án đánh số, năm ô
  một hàng**; short answer có dòng kẻ dưới mỗi câu; summary/note in nguyên
  chỗ trống `(n)______`. Đáp án và transcript nằm ở trang riêng. Times New
  Roman 13. File được dựng ngay trong trình duyệt (docx-rs thuần Rust, không
  kéo theo thư viện ảnh), không tốn một request nào lên server.
- **Markdown và DOCX chỉ dựng khi bấm.** Trước đây chuỗi Markdown được dựng
  lại ở mỗi lần re-render, kể cả khi đang poll tiến độ. Giờ cả hai chỉ chạy
  trong `onclick`.

### 🧰 Bên trong

- Bảng `exams` nằm chung `jobs.db`: JSON của `SavedExam` (đề + topic từng
  phần + job ghi âm + cờ stale) cộng các cột tóm tắt để liệt kê mà không phải
  parse JSON. Bốn server function: `save_exam`, `list_exams`, `load_exam`,
  `delete_exam`; body giới hạn 2 MB, tiêu đề 200 ký tự, chủ đề 2.000 ký tự.
  `load_exam` tra lại bảng job để trả `AudioTrack` mới nhất.
- Xoá đề chạy trong một transaction: xoá hàng đề, rồi xoá job và file WAV nếu
  job đã xong và không đề nào khác tham chiếu; job còn chạy được để lại cho
  retention dọn sau.
- `export/docx.rs` có `answer_layout` match **toàn bộ** `TaskKind`, nên thêm
  loại câu hỏi mới mà quên chỗ ghi đáp án là không biên dịch được (quy tắc
  "compiler lists every place" giờ đúng cả với export).
- Giờ hiển thị theo múi giờ trình duyệt (`js-sys Date`), server render dùng
  UTC dự phòng.

### 📊 Con số biết nói

| Kiểm tra | Kết quả |
|---|---|
| `cargo check` web / server / wasm32 | sạch, **0 warning** cả ba |
| `cargo test --features server --no-default-features` | **53/53** (0.5.0: 42, thêm 11 test: retention, đường dẫn WAV, store đề, ghim/xoá audio, DOCX, đồng hồ) |
| Trên trình duyệt thật (`dx serve`, key giả) | lưu tay, tự lưu sau 1 s, New exam, Open, Delete hai lần bấm: ✅ |
| Khôi phục audio từ hàng job có đường dẫn tuyệt đối của máy khác | `/audio/fixture` vẫn trả 206 + Content-Range, tải đủ 144.044 byte |
| Mở đề khi job ghi âm còn chạy, rồi job xong | popup tiến độ → player xuất hiện sau vòng poll, đề tự lưu lại |
| DOCX tải từ wasm | 68.699 byte, đúng từng byte với bản sinh trên server; Word mở được, xuất PDF 6 trang |
| `AUDIO_RETENTION_HOURS=abc` | server thoát mã 1, nêu tên biến |
| WASM release (chưa qua wasm-opt, lỗi crash trên Windows như 0.5.0) | 2,67 MB → **3,79 MB** (+1,1 MB vì docx-rs) |
| Tiền Gemini tốn cho toàn bộ kiểm tra | **0 đồng** |

### Thêm

- `src/application/exams.rs`, `src/infrastructure/exams.rs`,
  `src/export/docx.rs`, `src/ui/components/exam_library.rs`, `src/ui/clock.rs`.
- Biến `AUDIO_RETENTION_HOURS` (`.env.example`, template portable, README).
- Dependency `docx-rs` 0.4.22 (`default-features = false`).

### Thay đổi

- Hàng `jobs.output_path` mới chỉ chứa tên file WAV; đọc qua
  `JobRecord::output_file`.
- Dọn dẹp bỏ qua job được đề đã lưu tham chiếu; `ensure_cleanup_running` đọc
  cấu hình thay vì hằng số 24 giờ.
- Panel "Paper, key and transcripts" có hai nút DOCX/Markdown; trang từng
  phần cũng vậy.
- Phiên bản app và launcher portable: 0.6.0.

### Biết rồi, để bản sau

- Trang từng phần (`/`) chưa lưu; trạng thái của nó không phải là một `Exam`.
- Hai tab cùng mở một đề: bản ghi sau thắng.
- Xoá đề khi bản ghi âm còn đang render: job bị bỏ ghim và chờ retention dọn;
  với `AUDIO_RETENTION_HOURS=0` thì hàng và file ở lại tới khi xoá tay.
- Chưa có đăng nhập: ai vào được server là thấy mọi đề đã lưu (roadmap mục 2).
- Bản Windows 0.6.0 (`.exe` 8,5 MB) đã build và qua `packaging/smoke.py` trên
  chính file phát hành. AppImage Linux 0.6.0 chưa build; bản 0.5.0 là bản Linux
  mới nhất.

## [0.5.0] – 2026-09-23 — 🚀 "Một file. Bấm đúp. Chạy."

Trước bản này, muốn dùng được app phải có Rust, Dioxus CLI hoặc Docker, tức là
một lập trình viên ngồi cạnh. **Từ 0.5.0 thì không cần nữa.** Giáo viên chỉ
cần **một file duy nhất**: `.exe` 7,4 MB cho Windows 10/11 hoặc `.AppImage`
11,4 MB cho Linux. Bấm đúp: server khởi động, trình duyệt tự mở, ra đề. Không
cài đặt, không cần quyền admin, không cài thêm phần mềm nào. Cấu hình và dữ
liệu nằm ngay cạnh file chạy; chép cả thư mục sang máy khác là dùng tiếp.

### ✨ Điểm nhấn

- **Hướng dẫn cài đặt gói gọn trong một câu.** Lần đầu bấm đúp, app tự tạo
  `.env` và `voices.json` cạnh file chạy, rồi chỉ đích danh dòng cần sửa:
  *"Edit .env, replace your_api_key_here with your key, then restart."* Điền
  key, bấm lại là trình duyệt mở `http://127.0.0.1:8080`.
- **Lỗi được báo rõ, không crash.** Dotenv sai cú pháp hay sai mã hoá, thiếu key,
  model/IP/PORT/RUST_LOG vớ vẩn, `voices.json` hỏng hoặc có tên giọng rỗng,
  file nhạc không phải WAV 24 kHz, thư mục data/log không ghi được, database
  chỉ đọc, cổng 8080 đã có người chiếm: **mỗi lỗi một câu, chỉ đúng file hoặc
  biến cần sửa, thoát với mã 1.** Không stack trace, không panic, và không
  bao giờ lộ nội dung `.env` hay giá trị key ra màn hình.
- **Không bao giờ ghi đè file của người dùng.** Template được tạo bằng
  `create_new` (độc quyền, kể cả khi hai lần chạy tranh nhau). File thiếu thì
  tạo lại; file có sẵn, kể cả file hỏng, được giữ nguyên từng byte để người
  dùng tự sửa.
- **Double-click cũng không bị mất lỗi.** Khi console tự mở rồi tự đóng, lỗi
  khởi động vẫn nằm trên màn hình cho tới khi nhấn Enter. Chạy từ terminal
  có sẵn hoặc với `--non-interactive` thì thoát ngay, không chờ.
- **Tắt kiểu nào cũng sạch.** Ctrl+C, Ctrl+Break hay bấm nút X của cửa sổ đều
  dừng server và trả cổng; trên Windows thư mục tạm luôn được dọn. Mở trình
  duyệt thất bại thì URL vẫn in ra, server vẫn chạy.
- **Mang đi đâu cũng chạy.** Đường dẫn có dấu cách và Unicode (`résumé`,
  `ư`), chạy từ thư mục khác, chuyển cả thư mục app sang chỗ mới: đều đã test.
  Đường dẫn tương đối trong `.env` luôn tính từ chỗ đặt file chạy, không phụ
  thuộc thư mục hiện hành.

### 🧰 Bên trong hai gói

- **Windows:** một launcher Rust nhỏ, console subsystem, CRT liên kết tĩnh,
  **chỉ gọi DLL hệ thống của Windows**. Bên trong chứa trọn `server.exe` và
  `public/` cùng phiên bản; khi chạy thì giải nén vào thư mục tạm riêng, gọi
  server với đúng thư mục gốc, trả lại mã thoát rồi dọn sạch.
- **Linux:** AppImage Type 2, chạy trên Ubuntu 22.04 trở lên (cần glibc từ
  2.34; Ubuntu 22.04 có 2.35). Mang theo libssl, libcrypto, libgcc_s, bộ CA
  dự phòng cho máy tối giản, icon, desktop entry và đầy đủ giấy phép. Không có
  FUSE thì dùng `APPIMAGE_EXTRACT_AND_RUN=1`.
- **Build lặp lại được:** `packaging/build-windows.ps1` build trên máy,
  `packaging/build-linux.ps1` build trong container Ubuntu 22.04 qua Podman.
  Rust 1.92.0, Dioxus CLI 0.7.9, lockfile và hash của appimagetool/runtime
  đều được ghim cố định. Script chỉ gửi vào builder một danh sách file cho
  phép, không bao giờ gửi `.env`, `.git` hay `data/`.

### 🕵️ Hậu trường: những bug bị tóm trước khi tới tay giáo viên

- **Mỗi lần tắt app bằng nút X bị mất 19,8 MB ổ đĩa.** Windows kết thúc
  launcher trước khi nó kịp dọn thư mục tạm. Giờ launcher giữ hạn chót của sự
  kiện close/logoff/shutdown cho tới khi dọn xong: 10/10 lần đóng, 0 byte sót.
  Nếu cửa sổ bị đóng ngay lúc đang giải nén, launcher không khởi động server
  nữa, nên không có server nào chạy ngầm giữ cổng 8080.
- **`voices.json` hỏng từng bị lặng lẽ bỏ qua**, giọng đọc tự quay về mặc định
  mà không ai biết. Giờ file được kiểm tra một lần lúc khởi động và báo đúng
  dòng, cột bị lỗi.
- **Database chỉ đọc từng chỉ lộ ra ở job đầu tiên.** Giờ lúc khởi động có
  một lần ghi thử rồi rollback, trước khi nhận request.
- **WAV bị cắt cụt** giờ bị từ chối gọn gàng thay vì có thể gây panic; có test
  thử cắt ở từng byte một.

### 📊 Con số biết nói

| Kiểm tra | Kết quả |
|---|---|
| `cargo check` web / server / wasm32 | sạch, **0 warning** cả ba |
| `cargo test --features server --no-default-features` | **42/42** (0.4.0: 28, thêm 14 test) |
| `smoke.py` trên file EXE thật, Windows 11 | ✅ đạt |
| `smoke.py` trên AppImage thật, Ubuntu **22.04** và **24.04** sạch, user `nobody` | ✅ đạt |
| Đóng cửa sổ console bằng nút X | 10/10 lần, 0 thư mục tạm sót lại |
| Double-click khi thiếu key | lỗi ở lại màn hình tới khi nhấn Enter, thoát mã 1 |
| Tự mở trình duyệt mặc định | ✅ Firefox mở và kết nối được |
| Nội dung gói | không có `.env`, key, bản ghi âm, database hay mã nguồn |
| Tiền Gemini tốn cho toàn bộ bộ test | **0 đồng** (key giả, fixture audio và SQLite cục bộ) |

Smoke test chạy trên chính file phát hành, trong đường dẫn có dấu cách và
Unicode. Nó kiểm tra lần chạy đầu, cấu hình hỏng, việc giữ file có sẵn, lỗi
quyền ghi data/log/database, cổng bị chiếm, `/` và `/exam`, JS/WASM/CSS, một
server function, tải WAV kèm HTTP Range, tắt êm, chuyển thư mục và
`--config-dir`. Biên bản đầy đủ nằm ở `docs/portable-verification-v0.5.0.md`.

### Thêm

- `src/infrastructure/startup.rs`: `--portable`, `--config-dir`, `--no-open`,
  `--non-interactive`; tự bind listener, mở trình duyệt sau khi khởi tạo xong,
  tắt êm.
- `tools/portable-launcher/`, `packaging/` (script build/test, Containerfile,
  AppRun, desktop entry, icon, giấy phép), `docs/portable.md`.

### Thay đổi

- Cấu hình và `voices.json` được kiểm tra một lần lúc khởi động, giữ trong bộ
  nhớ; sửa file thì khởi động lại.
- SQLite và log được khởi tạo trước khi nhận request; `JobStore::global` không
  còn tự mở database giữa chừng.
- Biến môi trường vẫn ghi đè giá trị trong file (Docker không phải đổi gì),
  nhưng file sai cú pháp luôn bị từ chối.
- `dx serve` vẫn hot reload như cũ; dev và Docker không tự mở trình duyệt.

### ⚠️ Thay đổi không tương thích

- Mọi chế độ server đều bắt buộc có `GEMINI_API_KEY` lúc khởi động. Key có
  hợp lệ với Google hay không vẫn chỉ được kiểm tra khi sinh đề.
- `.env` chỉ được đọc từ thư mục cấu hình đã chọn, không còn tìm ngược lên thư
  mục cha.

### Biết rồi, để bản sau

- Chưa lưu đề khi đóng trình duyệt; audio vẫn hết hạn sau 24 giờ.
- WASM chưa được `wasm-opt` nén (~2,7 MB, phục vụ từ localhost nên gần như
  không ảnh hưởng): bản binaryen mà dx tải về bị crash trên Windows.
- Chưa test mount FUSE thật hay mở trình duyệt trên desktop Linux, chưa test
  Windows 10. Chưa ký số, chưa có ARM64, chưa tự cập nhật.

## [0.4.0] – 2026-09-22 — "Trọn một đề, một file WAV"

Roadmap mục 1 đã xong: trang **`/exam`** nhận một chủ đề cho mỗi part, một
nút bấm cho ra **cả đề thi** (câu hỏi bốn part, đáp án, transcript) và **một
bản ghi âm hoàn chỉnh** theo `AudioProgram` (nhạc, hiệu lệnh, thông báo, 20 s
đọc đề, phát lại cho part phát hai lần, thời gian kiểm tra). Bốn script sinh
song song; rồi câu hỏi của từng part sinh song song với job audio duy nhất.
File WAV (~86 MB cho 30 phút) không còn đi qua server-fn dưới dạng JSON mà
được **stream qua `/audio/{job_id}`**, tua được, tải thẳng.

### Điểm nhấn

- **Một đề, một nút.** `ui/views/exam.rs` giữ `ExamState { exam: domain::Exam,
  work, audio, run }`; `domain::Exam` là nguồn sự thật, `render_exam` và
  `ExamAudioRequest` tiêu thụ trực tiếp. State nằm trong context của layout
  `Navbar` và pipeline chạy bằng `spawn_forever`, nên chuyển trang không làm
  rơi job 25 phút.
- **Route stream audio là handler axum thường, cố ý không phải server-fn.**
  dioxus-server bọc mọi server-fn và ép redirect 302 → Referer khi request
  chấp nhận `text/html`, đúng thứ thẻ `<a download>` gửi; `FileStream` lại
  gán `.wav` thành `text/html`. `infrastructure/jobs/serve.rs` dùng
  `tower_http::services::ServeFile`: `audio/wav`, `Accept-Ranges`, 206 cho
  `Range`. `main.rs` gắn nó qua `dioxus::serve(|| async { Ok(router(App)
  .route(AUDIO_ROUTE, get(serve_audio))) })`.
- **Job audio cả đề đọc hai part cùng lúc** (`Semaphore(2)` +
  `try_join_all`) thay vì tuần tự, và báo tiến độ 0,1 → 0,8 theo số part
  xong; UI hiện "reading part k of n" rồi "assembling…".
- **`validate_exam`** trong domain: part nào chưa có script, task nào còn
  thiếu, số câu có liên tục 1..=35/40 không. Nút tải luôn bật, nhưng đổi
  nhãn thành "Download draft (incomplete)" và liệt kê lý do phía trên.

### Thêm

- `src/ui/views/exam.rs` (`ExamView`, `ExamState`, `PartWork`, `AudioWork`,
  `Step`), route `/exam`, link "Whole exam" trên navbar.
- `src/infrastructure/jobs/serve.rs` (`serve_audio`), `AUDIO_ROUTE` và
  `audio_url` trong `application/audio.rs`.
- `JobView.track: Option<AudioTrack>`: `audio_job_status` dựng `AudioTrack`
  (container, 24 kHz, thời lượng từ kích thước file, `location` = job id)
  khi job xong. `AudioTrack` lần đầu có nơi khởi tạo.
- `wav::duration_ms_for_len`, `validate_exam`, 5 test mới (28 tổng).
- `src/ui/components/issue_list.rs` (dùng chung hai trang).
- Dependency `tower-http` (feature `fs`, phía server).

### Thay đổi

- `main.rs` dùng `dioxus::serve` với router tuỳ chỉnh (server) và
  `dioxus::launch` (browser); `ensure_cleanup_running` chạy từ lúc boot thay
  vì từ request đầu tiên.
- `AudioPlayerSection` nhận URL thay vì `Vec<u8>`: `<audio src>` và
  `<a download>` trỏ thẳng vào `/audio/{job_id}`; trang part cũng dùng đường
  này (WAV 14 MB của một part trước đây thành ~50 MB JSON đi qua server-fn).
- `ui/jobs.rs` thêm nhịp/trần cho job cả đề (5 s, tối đa 45 phút).

### Bỏ

- `audio_job_result` (trả `Vec<u8>` qua server-fn).

### Đã kiểm chứng

| Kiểm tra | Kết quả |
|---|---|
| `cargo check` (web) | sạch, 0 warning |
| `cargo check --target wasm32-unknown-unknown` | sạch |
| `cargo check --features server --no-default-features` | sạch, 0 warning |
| `cargo test --features server --no-default-features` | 28/28 |

### Còn phía trước

Lưu đề vào SQLite (để lấy lại bản ghi sau khi đóng tab); DOCX; MP3; xác
thực; sinh lại từng câu; chỉnh giọng từng part trên trang `/exam`.

## [0.3.0] – 2026-09-22 — "Một nút, trọn một part"

Trước đây giáo viên bấm ba lần cho một part: script, rồi câu hỏi, rồi audio;
transcript chỉ xuất hiện khi đã có câu hỏi. Bản này thêm **một nút sinh trọn
gói**: script trước, rồi câu hỏi và bản ghi âm chạy **song song**, transcript
tải được ngay khi có script. Toàn bộ điều phối nằm ở trình duyệt, nối đúng
các server-fn đã có; phía server không đổi một dòng.

### Điểm nhấn

- **Một lần bấm, năm sản phẩm.** "Generate script, questions and audio" gọi
  `generate_passage`, rồi `start_part_audio` (job TTS chạy nền ngay) và vòng
  `generate_task` cùng lúc bằng `futures_util::future::join`. Câu hỏi và TTS
  chỉ phụ thuộc vào `Passage` bất biến nên chạy song song là an toàn; bản
  ghi âm vốn là đường găng, câu hỏi xong trong lúc chờ nó.
- **Script có lỗi thì dừng.** Nếu validator trả issue mức Error (nhãn giọng
  lạ, chỗ trống trong script) pipeline dừng sau bước script và hiện lý do;
  cảnh báo (độ dài) thì đi tiếp. Cùng lỗi đó sẽ làm hỏng TTS và grounding.
- **Huỷ là huỷ thật ở phía trình duyệt.** `HomeState.run` tăng mỗi lần đặt
  lại kết quả hoặc huỷ; mọi kết quả về muộn của lần chạy cũ bị bỏ thay vì đè
  lên kết quả mới. Job TTS đã khởi động vẫn chạy trên server; id được giữ để
  "Check again" lấy về sau.

### Thêm

- `src/ui/jobs.rs`: `wait_for_job` poll `audio_job_status` theo nhịp và trần
  thời gian của từng loại job (part: 2 s, tối đa 15 phút; trước là 5 phút,
  không đủ cho part ba giọng đọc từng lượt).
- Nút "Download transcript (Markdown)" ngay dưới script.
- Nút "Check again" khi bản ghi âm chưa về (hết giờ chờ hoặc đã huỷ).
- Dependency `futures-util` (đã có sẵn trong lock qua Dioxus).

### Thay đổi

- Ba closure `handle_generate_*` trong `home.rs` thành ba hàm async dùng lại
  (`run_script`, `run_tasks`, `run_audio`); pipeline và các nút riêng cùng
  gọi chúng.
- Bốn popup tiến độ gộp thành một, nội dung theo trạng thái ("Writing
  questions (1 of 3) and recording the audio...").
- Nút "Generate script" thành "Script only", đứng cạnh nút sinh trọn gói.
- Đổi format hoặc part cũng vô hiệu hoá lần chạy đang dở.

### Đã kiểm chứng

| Kiểm tra | Kết quả |
|---|---|
| `cargo check` (web) | sạch, 0 warning |
| `cargo check --target wasm32-unknown-unknown` | sạch |
| `cargo check --features server --no-default-features` | sạch, 0 warning |
| `cargo test --features server --no-default-features` | 23/23 |

### Còn phía trước

Trang `/exam`: cả đề, bốn part song song, một bản ghi âm theo `AudioProgram`,
tải đề + đáp án + transcript trong một file.

## [0.2.0] – 2026-09-21 — "Từ máy đọc script IELTS thành máy ra đề nghe"

Bản này không phải một tính năng. Nó là một lần **lột xác toàn bộ**: 4.364 dòng
Rust được sắp lại thành bốn tầng, miền nghiệp vụ được mô hình hoá lại từ đầu,
và phạm vi sản phẩm đổi từ "sinh script + audio cho IELTS" sang **"sinh trọn
một đề thi nghe cho bất kỳ định dạng nào mô tả được bằng dữ liệu"**. Sinh ra
từ một buổi tối bị sếp dí đề HSG Quốc gia, kết thúc bằng một kiến trúc mà
người tiếp theo mở repo ra có thể đọc `docs/architecture.md` rồi làm việc
ngay.

### Điểm nhấn

- **Định dạng đề là dữ liệu, không phải code.** `ExamFormat` mô tả trọn một
  kỳ thi: từng part là loại băng nào (hội thoại, phỏng vấn, độc thoại, trích
  đoạn), phát mấy lần, dài bao nhiêu, giọng mặc định nào, và chuỗi task với
  dải câu hỏi. Hai preset đi kèm: **IELTS Listening** (40 câu) và **HSG Quốc
  gia – Listening** (35 câu, chép đúng từ đề chính thức 2025–2026: Part 1
  phỏng vấn 3 giọng phát 1 lần, Part 3–4 phát 2 lần, 2 phút kiểm tra). Thêm
  một định dạng mới là thêm một giá trị, không thêm một nhánh `match`.
- **Chín loại câu hỏi, một validator không nương tay.** T/F/NG, "ai nói",
  chọn 2/5 và 3/7, MCQ, trả lời ngắn, điền summary, điền note/form, hoàn
  thành câu, matching. Mọi đáp án điền từ **phải xuất hiện nguyên văn trong
  script** sau chuẩn hoá, đúng giới hạn từ, đúng luật số; mọi đáp án chữ cái
  phải nằm trong lựa chọn; chọn nhiều phải đủ đúng số chữ cái khác nhau; đánh
  số phải liền mạch. Kết quả sinh ra luôn là **draft kèm issues**, không bao
  giờ giấu lỗi để giáo viên tự dò.
- **Audio cả đề, đúng nghi thức phòng thi.** `AudioProgram` suy từ định
  dạng: nhạc mở, âm báo, lời dẫn từng part, nghỉ đọc đề, phát băng, phát lại
  nếu part đó phát hai lần, thông báo hết giờ, khoảng lặng kiểm tra, nhạc
  đóng. Ghép WAV, sinh tone, đọc/ghi RIFF **không cần một crate audio nào**.
- **Xử lý giới hạn thật của Gemini thay vì giả vờ không có.** TTS đa giọng
  của Gemini chỉ nhận 2 giọng và 8.192 token input mỗi request; phỏng vấn
  host + 2 khách hoặc script dài được tự động đọc theo lượt rồi ghép với
  khoảng nghỉ. API key đi qua header `x-goog-api-key`, không bao giờ nằm
  trong URL để lọt vào log.
- **Một client Gemini thay cho ba bản copy.** Cùng một chính sách retry
  (429/503/timeout, backoff luỹ thừa), JSON mode có fallback nếu server đổi
  field, lỗi trả về là câu tiếng người chứ không phải mã HTTP.

### Kiến trúc

Bốn tầng, phụ thuộc một chiều, được compiler bảo vệ:

```
ui → application → { domain, infrastructure }
infrastructure → domain
export → domain
```

- `src/domain/` — thuần Rust, không Dioxus/tokio/reqwest/sqlx, compile y hệt
  cho wasm và server, unit test bằng `cargo test` thường.
- `src/application/` — nơi **duy nhất** có `#[server]`. Tận dụng việc Dioxus
  0.7 tự cắt body server fn khỏi bundle web, bỏ hẳn pattern
  `#[cfg(feature = "server")]` lồng trong body của bản cũ.
- `src/infrastructure/` — chỉ build ở server: config, Gemini, prompt, TTS,
  WAV/chương trình audio, job SQLite, rate limit.
- `src/export/` — render Markdown giấy thi, đáp án, transcript.
- `src/ui/` — component và view Dioxus.

### Thêm

- Miền: `ExamFormat`, `PartSpec`, `TaskSpec`, `TaskKind`, `WordLimit`,
  `PlayCount`, `PassageKind`, `Passage`/`Line` (có parser chịu được markdown
  lạc vào), `Task`/`Item`/`Choice`/`Answer` (JSON gắn thẻ `kind/value` để mô
  hình ngôn ngữ trả về đúng hình dạng), `Exam`/`ExamPart` với `answer_key()`,
  `AudioProgram`/`AudioSegment`, `ValidationIssue` có mức độ.
- Command tự kiểm tra: `PassageRequest`, `TaskRequest`, `AudioRequest`,
  `ExamAudioRequest`.
- Server fn: `suggest_topic`, `generate_passage`, `generate_task`,
  `start_part_audio`, `start_exam_audio`, `audio_job_status`,
  `audio_job_result`.
- Prompt theo từng loại task, yêu cầu trích dẫn **evidence nguyên văn** cho
  mỗi câu để validator đối chiếu.
- `voices.json` có thêm giọng announcer; file được tạo với giá trị mặc định
  nếu thiếu.
- Job kind `exam_audio` render cả đề chạy nền; dọn dẹp mỗi giờ, giữ 24 h.
- 23 unit test cho domain, export, WAV, chương trình audio, client.
- Tài liệu viết lại toàn bộ: `docs/architecture.md` (sơ đồ, module map,
  pipeline, ràng buộc TTS, roadmap), `docs/domain_model.md`,
  `docs/ubiquitous_language.md` (có bảng thuật ngữ đã khai tử),
  `docs/project_scope.md`, `README.md`.
- Bảng đối chiếu tài liệu Google ngày 21/09/2026: tình trạng model, giới hạn
  token, giá, free tier, nhãn "Legacy" của `generateContent`.

### Thay đổi

- Giao diện chọn **định dạng** rồi **part**, hiển thị loại băng, số lần phát
  và danh sách task; nút sinh câu hỏi và tải "giấy thi + đáp án + transcript"
  dạng Markdown.
- `SpeakerConfig.name` đổi thành `label` ("Speaker A") để tách nhãn lượt nói
  khỏi tên nhân vật; thêm vai Host, Expert, Guest, Reporter, Narrator.
- Tên model tập trung tại `config.rs`: text `gemini-flash-latest` (alias cố
  ý không pin), TTS `gemini-2.5-pro-preview-tts`; override bằng biến môi
  trường.
- Bảng job SQLite đổi tên `jobs`, có cột `kind`.
- Dockerfile viết lại: `rust:1.92` (edition 2024 cần ≥ 1.85), `dioxus-cli
  0.7.9`, đường dẫn output `target/dx/vmq_mvp/release/web/{server,public}`
  đã xác minh bằng build thật. Bản cũ dùng `rust:1.80` và `dioxus-cli 0.6.1`
  cho crate Dioxus 0.7 nên **chưa từng build được**.
- `docker-compose.yml` bind `127.0.0.1:8080`, đọc `.env`; README nói thẳng
  phải đặt reverse proxy có TLS và xác thực phía trước.
- Agent brief trong `.github/agents` đổi tên và viết lại theo phạm vi mới.

### Bỏ

- `ListeningSection`, `Section1..4`, `GenerationRequest`, `ListeningScript`,
  `GenerationResult`, `GenerationFailure`.
- `src/services/*` (thay bằng `application/` + `infrastructure/`),
  `src/components/*` và `src/views/*` (chuyển vào `src/ui/`).
- Ba bản copy của vòng retry Gemini; API key trong query string.
- Luật "sản phẩm này không sinh câu hỏi" trong tài liệu. Sinh câu hỏi có đáp
  án chính là mục đích.

### Đã kiểm chứng

| Kiểm tra | Kết quả |
|---|---|
| `cargo check` (web) | sạch, 0 warning |
| `cargo check --features server --no-default-features` | sạch, 0 warning |
| `cargo test --features server --no-default-features` | 23/23 |
| `dx build --release` | server + `public/` đúng layout Dockerfile |

### Còn phía trước

Theo thứ tự trong `docs/architecture.md`: UI cấp đề gọi `start_exam_audio`
và tải cả đề; lưu đề vào SQLite; xuất DOCX đúng layout giấy thi; MP3 qua
`ffmpeg`; xác thực trước khi lên VPS; sinh lại từng câu với issues của
validator đưa ngược vào prompt.

## [0.1.0] – 2026-01

Bản MVP đầu tiên: sinh script IELTS Listening Section 1–4 bằng Gemini, đọc
thành WAV bằng Gemini TTS đa giọng, hàng đợi job SQLite, giao diện Dioxus
fullstack. Cố ý không sinh câu hỏi.
