# Changelog

Mọi thay đổi đáng kể của dự án được ghi ở đây. Định dạng theo tinh thần
[Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), phiên bản theo SemVer.

## [0.5.0] – 2026-09-23 — Bản portable cho Windows và Linux

Ứng dụng chạy được như một file duy nhất: **EXE cho Windows 10/11 x64** và
**AppImage cho Linux x86-64** (nền Ubuntu 22.04). Người dùng không cần Rust,
Dioxus CLI hay Docker. Bấm chạy thì server cục bộ khởi động và trình duyệt mở
`http://127.0.0.1:8080`. Cấu hình và dữ liệu nằm cạnh file gốc.

### Điểm nhấn

- **Lần chạy đầu tạo `.env` và `voices.json`** cạnh file EXE/AppImage (tạo
  độc quyền, không bao giờ ghi đè), rồi dừng với hướng dẫn điền
  `GEMINI_API_KEY`. File thiếu được tạo lại; file có sẵn được giữ nguyên.
- **Lỗi khởi động rõ ràng, không panic.** Sai cú pháp dotenv, sai mã hoá,
  thiếu key, model/IP/PORT/RUST_LOG không hợp lệ, `voices.json` hỏng hoặc có
  tên giọng rỗng, file nhạc sai định dạng, thư mục data/log hoặc SQLite không
  ghi được, cổng đã bị chiếm: đều báo lỗi kèm đường dẫn/biến liên quan và thoát
  với mã khác 0. Thông báo lỗi không bao giờ chứa nội dung `.env` hay giá trị key.
- **EXE Windows là launcher nhỏ viết bằng Rust** chứa nguyên server và
  `public/` khớp phiên bản. Launcher giải nén vào thư mục tạm riêng, chạy server
  với thư mục cấu hình gốc, trả lại mã thoát và dọn thư mục tạm, kể cả khi người
  dùng đóng cửa sổ console bằng nút X. Khi chạy bằng double-click, lỗi khởi động
  được giữ trên màn hình cho tới khi nhấn Enter.
- **AppImage Type 2** chứa server, tài sản web, các thư viện không thuộc hệ
  thống (libssl, libcrypto, libgcc_s), icon, desktop entry, CA fallback và giấy
  phép. `AppRun` dùng `APPIMAGE` để tìm thư mục cấu hình cạnh file gốc.

### Thêm

- `src/infrastructure/startup.rs`: tuỳ chọn `--portable`, `--config-dir`,
  `--no-open`, `--non-interactive`; listener tự quản lý, mở trình duyệt sau
  khi khởi tạo xong (nếu không mở được thì vẫn in URL và server vẫn chạy),
  tắt êm bằng Ctrl+C.
- `tools/portable-launcher/` (launcher Windows), `packaging/` (script build
  PowerShell/Linux, Containerfile Ubuntu 22.04, AppRun, smoke test
  `smoke.py` chạy trên gói thật không gọi Gemini, `test-linux.ps1` cho
  Ubuntu 22.04/24.04, giấy phép thư viện).
- Test mới cho cấu hình lần đầu, file thiếu một phần, giữ file có sẵn,
  dotenv/JSON hỏng, sai mã hoá, đường dẫn tương đối, không lộ key, WAV bị
  cắt cụt, SQLite chỉ đọc (42 test tổng).
- `docs/portable.md`, `docs/portable-verification-v0.5.0.md`.

### Thay đổi

- Cấu hình và `voices.json` được kiểm tra một lần lúc khởi động và giữ trong
  bộ nhớ; sửa file thì phải khởi động lại. Không còn lặng lẽ quay về giọng mặc
  định khi file hỏng.
- SQLite và log được khởi tạo (kèm một lần ghi thử rồi rollback) trước khi
  nhận request. `JobStore::global` không còn tự mở database.
- Biến môi trường vẫn ghi đè giá trị trong file (tương thích Docker), nhưng
  file cấu hình sai cú pháp luôn bị từ chối.
- `dx serve` (debug) vẫn đi qua `dioxus::serve` nên hot reload giữ nguyên;
  dev và Docker không tự mở trình duyệt.

### Thay đổi không tương thích

- Mọi chế độ server đều bắt buộc có `GEMINI_API_KEY` lúc khởi động (tính hợp
  lệ với Google vẫn chỉ được kiểm tra khi sinh đề).
- `.env` chỉ được đọc từ thư mục cấu hình đã chọn, không còn tìm ngược lên thư
  mục cha.

### Chưa làm trong bản này

- Chưa lưu đề qua lần đóng trình duyệt; audio vẫn hết hạn sau 24 giờ.
- Chưa ký số, chưa có ARM64, chưa tự cập nhật.

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
