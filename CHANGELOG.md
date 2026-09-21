# Changelog

Mọi thay đổi đáng kể của dự án được ghi ở đây. Định dạng theo tinh thần
[Keep a Changelog](https://keepachangelog.com/vi/1.1.0/), phiên bản theo SemVer.

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
