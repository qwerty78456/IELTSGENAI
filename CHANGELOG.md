# Changelog

Mọi thay đổi đáng kể của dự án được ghi ở đây. Định dạng theo tinh thần
[Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), phiên bản theo SemVer.

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
