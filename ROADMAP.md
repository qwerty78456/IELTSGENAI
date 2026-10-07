# ROADMAP — Máy chủ Windows luôn bật, mở ra Internet qua Cloudflare Tunnel

Kế hoạch triển khai: chạy app thành Windows service bằng NSSM, tự chạy lại sau
khi khởi động lại máy, mất điện hay crash; mở cho giáo viên ngoài mạng qua
Cloudflare Tunnel với domain `ielts-ai-master.co.uk`, có Cloudflare Access đứng
trước. **0.9.0 (2026-10-07) phát hành phần sửa app (G1)**; installer service,
gia cố `cloudflared` và lên sóng (G2–G5) đi ở bản sau.

Lộ trình **sản phẩm** (MP3, đăng nhập, tạo lại một câu…) vẫn ở
[`docs/architecture.md`](docs/architecture.md#roadmap-in-order). File này chỉ
theo dõi việc triển khai. Khi xong, mục 2 của lộ trình đó ("Authentication …
or Cloudflare Access") được giải quyết một nửa: Access chặn người lạ, app vẫn
chưa biết từng người dùng (xem B4).

Cập nhật lần cuối: **2026-10-07**.

---

## Tracking

Ký hiệu: ⬜ chưa làm · 🔄 đang làm · ✅ xong · ⏸️ hoãn · ❓ chờ quyết định.
Người làm: **Dev** là việc trong repo (code, script, tài liệu); **PO** là việc
cần tài khoản hoặc máy thật của bạn (dashboard Cloudflare, BIOS, quyền admin).

| Giai đoạn | Nội dung | Xong / Tổng | Trạng thái |
|---|---|---|---|
| G0 | Quyết định trước khi làm | 6 / 6 | ✅ |
| G1 | Sửa app (bắt buộc trước khi mở ra Internet) | 7 / 8 | ✅ (A4 đo sau khi lên sóng) |
| G2 | Đóng gói và cài Windows service (NSSM) | 0 / 7 | ⬜ |
| G3 | Gia cố `cloudflared` đang chạy | 0 / 5 | ⬜ |
| G4 | Mở app qua Tunnel + Access | 0 / 7 | ⬜ |
| G5 | Máy luôn bật | 1 / 5 | 🔄 |
| G6 | Phát hành 0.9.0 (phần app) | 5 / 5 | ✅ |

Thứ tự bắt buộc: **G1 → G2 → G4**. Không bật route ở G4 khi G1-A1 chưa xong
(xem lý do ở A1). G3 và G5 làm song song lúc nào cũng được, nhưng G3 nên xong
trước G4.

### Danh sách chi tiết

| ID | Việc | Người | Phụ thuộc | Trạng thái | Ghi chú |
|---|---|---|---|---|---|
| Q1 | Chọn hostname cho app | PO | — | ✅ | `app.ielts-ai-master.co.uk`. CNAME do route tunnel tự tạo (P3), không tạo tay |
| Q2 | Danh sách email được vào, cách đăng nhập | PO | — | ✅ | Chỉ Cloudflare Access (One-time PIN), chưa làm đăng nhập trong app (B4) |
| Q3 | Dữ liệu nào mang sang service | PO | — | ✅ | Bắt đầu trống (bỏ S5). Thiếu key thì hỏi người dùng (N1); đụng instance khác thì hỏi có tắt không (N2) |
| Q4 | Giáo viên ở xa có cần Voice Design không | PO | — | ✅ | Có: tạo giọng mở cho người vào qua tunnel; xoá giọng vẫn chỉ tại máy (V1) |
| Q5 | Số phiên bản | PO | — | ✅ | Chốt lúc phát hành |
| Q6 | Hãng/model bo mạch chủ | PO | — | ✅ | Installer tự đọc và ghi comment vào `.env`; máy ở nhà là bo Gigabyte; VPS bỏ qua M1 |
| A1 | Quy tắc "máy cục bộ" theo từng request + cổng riêng cho tunnel | Dev | — | ✅ | 0.9.0: `ingress` (Local / Published / Remote), `PUBLIC_PORT` + `PUBLIC_HOST` |
| A2 | Job ghi âm bị bỏ dở khi server khởi động lại | Dev | — | ✅ | 0.9.0: thành failed lúc khởi động, chỉ khi giữ khoá thư mục dữ liệu |
| A3 | `Cache-Control` cho audio và mẫu giọng | Dev | — | ✅ | 0.9.0: `private, no-cache` (không phải `no-store`: trình duyệt vẫn hỏi lại bằng 304, khỏi tải lại 87 MB) |
| A4 | Theo dõi giới hạn 125 giây (lỗi 524) | Dev | P7 | ⬜ | Đo 2 tuần sau khi lên sóng, sửa sau (B2) |
| V1 | Tạo giọng (Voice Design) cho người vào qua tunnel; xoá giọng chỉ tại máy | Dev | A1 | ✅ | 0.9.0; xoá giọng có bucket riêng `VoiceDelete` |
| F1 | Cờ khởi động `--service NAME` (không hỏi, không mở trình duyệt) | Dev | — | ✅ | 0.9.0; dùng trong lệnh NSSM |
| N1 | Thiếu key thì hỏi ngay trên console, ẩn ký tự, kiểm tra với Google, ghi `.env` | Dev | F1 | ✅ | 0.9.0: che `*`, tối đa 3 lần; không hỏi với `--service`, `--non-interactive`, session 0, `dx serve`; `.env` chỉ chủ file đọc được (NTFS) |
| N2 | Cổng bị app khác của mình chiếm: hỏi [y/N] có tắt không; tắt service thì tự bật lại khi bản chạy tay thoát | Dev | A1, F1 | ✅ | 0.9.0; nhánh service chưa chạy thật (kiểm ở S6); bật lại chỉ khi người dùng chưa đăng xuất (B12) |
| S1 | `packaging/install-service.ps1` (cài, nâng cấp, gỡ) | Dev | A1, F1 | ⬜ | Hỏi key ẩn ký tự, đọc phần cứng vào `.env`, cho người dùng tương tác quyền start/stop service (N2), ghi `PUBLIC_PORT` + `PUBLIC_HOST`, kiểm dải cổng bị Windows giữ (S8) |
| S2 | `packaging/service-smoke.ps1` kiểm thử service thật | Dev | S1 | ⬜ | |
| S3 | `docs/windows-service.md` | Dev | S1 | ⬜ | Tiếng Anh như các docs khác |
| S4 | Xoá service cũ `VMQ-MVP` | PO | — | ⬜ | Cần quyền admin |
| S5 | Chuyển dữ liệu sang `C:\ProgramData\ListeningExamGenerator` | PO + Dev | Q3, S1 | ⏸️ | Bỏ: service bắt đầu trống (Q3) |
| S6 | Cài service trên máy này và chạy S2 | PO + Dev | S1–S4 | ⬜ | |
| S7 | Ghi kết quả vào `docs/service-verification-v<bản>.md` | Dev | S6 | ⬜ | |
| S8 | Đưa dải cổng động TCP về mặc định 49152–65535 | PO | — | ⬜ | **Chặn S6.** Máy này đang để từ 1024, nên Hyper-V/WinNAT giữ 7610–8109, gồm cả 8080 và 8081. Cần quyền admin và khởi động lại (lệnh ở G2) |
| C1 | Cập nhật `cloudflared` ≥ 2026.7.3 | PO | — | ⬜ | Bản đang cài cũ hơn |
| C2 | Đổi (rotate) token tunnel | PO | C1 | ⬜ | Token mới chỉ nằm trong file có ACL chặt |
| C3 | Cài lại service `cloudflared` với token mới (token-file) | PO | C2 | ⬜ | |
| C4 | Recovery action cho service `cloudflared` | PO | C3 | ⬜ | |
| C5 | (Tuỳ chọn) Ghi log `cloudflared` ra file | PO | C3 | ⬜ | |
| P1 | Bật One-time PIN / Google trong Identity providers | PO | Q2 | ⬜ | |
| P2 | Tạo Access application cho hostname | PO | Q1, Q2, P1 | ⬜ | **Trước P3** |
| P3 | Thêm route Published application → `http://127.0.0.1:8081` | PO | A1, S6, P2 | ⬜ | Bật "Protect with Access"; `.env` có `PUBLIC_PORT=8081`, `PUBLIC_HOST=app.ielts-ai-master.co.uk` |
| P4 | Chỉnh cài đặt zone | PO | — | ⬜ | Tắt Email Obfuscation, Rocket Loader, Bot Fight Mode |
| P5 | Bật DNSSEC | PO | — | ⬜ | Có hiệu lực sau 1–2 ngày |
| P6 | Bật cảnh báo Tunnel Health Alert | PO | — | ⬜ | Miễn phí |
| P7 | Kiểm thử đầu-cuối từ điện thoại 4G | PO + Dev | P3, P4 | ⬜ | |
| M1 | BIOS: Restore on AC Power Loss = Power On | PO | Q6 | ⬜ | Bo Gigabyte: Settings → Platform Power → **AC BACK = Always On** (xem lại tên mục trên BIOS) |
| M2 | Windows Update: Active hours trùng giờ dạy | PO | — | ⬜ | |
| M3 | Thử Restart máy, không đăng nhập | PO + Dev | S6, C3 | ⬜ | |
| M4 | Thử rút điện rồi cắm lại | PO | M1, M3 | ⬜ | |
| M5 | Kiểm tra lại cài đặt nguồn | Dev | — | ✅ | Fast Startup tắt, sleep/hibernate = never (2026-10-05) |
| R1 | Cập nhật tài liệu (README, `.env.example`, CLAUDE.md, AGENTS.md, architecture.md, portable.md) | Dev | A1–A3 | ✅ | 0.9.0; phần service (S3) đi cùng installer |
| R2 | CHANGELOG 0.9.0 với bảng "Con số biết nói" | Dev | R3 | ✅ | Song ngữ như các bản trước |
| R3 | Kiểm tra phát hành (fmt, hai `cargo check`, tests, smoke portable) | Dev | A1–A3 | ✅ | `docs/portable-verification-v0.9.0.md` |
| R4 | Nâng phiên bản trong `Cargo.toml` | Dev | Q5 | ✅ | 0.9.0 |
| R5 | Commit / tag / GitHub release | Dev | R1–R4 | ✅ | PO yêu cầu 2026-10-07 |

---

## Hiện trạng đã khảo sát (2026-10-05)

Máy: **Windows 11 IoT Enterprise LTSC 2024**, build 26100. Không có sản phẩm
"Windows Server 2024": bản server hiện hành là **Windows Server 2025** (GA
2024-11-01), cùng nền build 26100, service chạy giống hệt nhau trên cả hai.

**NSSM**

- Đã cài `nssm` **2.24-101-g897c7ad** qua Chocolatey (gói 2.24.101.20180116),
  ở `C:\ProgramData\chocolatey\bin\nssm.exe`. Đây là bản cần dùng: bản 2.24
  gốc (2014) lỗi trên Windows 10 1703 trở lên, và tài khoản ảo
  (`NT SERVICE\…`) chỉ có từ các bản pre-release.
- Có service NSSM cũ **`VMQ-MVP`**: Disabled, chạy bằng LocalSystem, trỏ tới
  một bản build cũ không còn trên máy, bind `0.0.0.0`. Không dùng lại: bind
  `0.0.0.0` để lộ app không đăng nhập ra mạng LAN.

**Cloudflare**

- `cloudflared` cài bằng MSI, chạy thành service `cloudflared` Automatic,
  LocalSystem, tunnel quản lý từ dashboard, kết nối khỏe. Bản đang cài cũ hơn
  2026.7.3; từ bản đó `service install` lưu token tunnel vào
  `C:\ProgramData\cloudflared\token` có ACL chặt thay vì trong cấu hình
  service. Cần cập nhật và cài lại (G3).
- Service `cloudflared` **không có recovery action** (`sc qfailure` trống) và
  không ghi log ra file.
- Tunnel đã có route khác, được Access bảo vệ, nên team Zero Trust đã tồn tại.
  Domain gốc chưa có route (trả 530).

**Nguồn điện và tắt máy**

- Fast Startup tắt (`HiberbootEnabled = 0`); sleep và hibernate khi cắm điện
  là never.
- `WaitToKillServiceTimeout = 5000` ms: khi tắt máy, mỗi service chỉ có khoảng
  5 giây để dừng.

**App (0.8.0)**

- Server là app console, đã bắt Ctrl+C và Ctrl+Break để tắt êm
  ([startup.rs:104](src/infrastructure/startup.rs:104)). NSSM dừng service bằng
  `CTRL_C_EVENT` gửi tới console riêng của app, nên khớp.
- Không có `--portable` thì app không mở trình duyệt và đọc `.env` trong thư
  mục `--config-dir` ([config.rs:146](src/infrastructure/config.rs:146)).
- API key hiện nằm trong biến môi trường của **người dùng** (HKCU). Service
  không đọc được HKCU của người dùng đó, nên phải đặt key vào `.env` của service.
- `local_server()` ([settings.rs:81](src/application/settings.rs:81)) chỉ xem
  địa chỉ bind. Xem A1.
- Job ghi âm chạy dở không được dọn khi server tắt. Xem A2.

---

## Các quyết định

| # | Quyết định | Lý do | Phương án đã loại |
|---|---|---|---|
| D1 | Dùng **NSSM 2.24-101 đang có** | Không cần sửa code để thành service; đã cài sẵn | WinSW (đứng yên từ 2023, cần .NET), Servy (.NET, GUI, nặng cho một EXE), Task Scheduler (không có tín hiệu dừng, tự dừng sau 72 giờ) |
| D2 | Chạy thẳng `server.exe` + `public\` trong `C:\Program Files\ListeningExamGenerator` | EXE portable giải nén payload vào `%TEMP%` mỗi lần chạy ([main.rs:79](tools/portable-launcher/src/main.rs:79)); bị NSSM kill thì để lại rác; còn tự mở trình duyệt | Chạy EXE portable dưới NSSM |
| D3 | Tài khoản ảo `NT SERVICE\ListeningExamGenerator` | Ít quyền hơn LocalSystem, không có mật khẩu phải quản lý | LocalSystem (service cũ dùng), tài khoản người dùng có mật khẩu |
| D4 | API key trong `.env` của thư mục cấu hình, ACL chỉ cho Administrators, SYSTEM và service | Service không thấy HKCU; `AppEnvironmentExtra` lưu trong registry mà Users đọc được; biến môi trường máy thì mọi tiến trình đều đọc được và lại thắng `.env` | `AppEnvironmentExtra`, biến môi trường máy |
| D5 | Tunnel trỏ vào **cổng riêng 8081**; cổng 8080 chỉ dùng tại máy; thêm lớp chặn theo header | Cổng riêng không phụ thuộc vào việc Cloudflare gửi header nào; header là lớp thứ hai nếu ai đó trỏ nhầm route vào 8080 | Chỉ kiểm tra header; tắt hẳn nhập key và Voice Design cho cả máy |
| D6 | Dùng lại **tunnel quản lý từ dashboard đang chạy** | Mỗi máy chỉ cài được một service `cloudflared` bằng `service install`; route sửa trên dashboard | Tunnel thứ hai; tunnel cục bộ bằng `config.yml` (phải chép file vào `systemprofile`, sửa ImagePath bằng tay) |
| D7 | **Access đứng trước app**, tạo Access application **trước** khi thêm route | App chưa có đăng nhập; mọi request đều tiêu tiền Gemini | Mở công khai rồi dựa vào rate limit |
| D8 | Bật **"Protect with Access"** ở route | `cloudflared` tự kiểm JWT của Access trước khi chuyển request vào app: lớp bảo vệ thứ hai mà không cần code | Kiểm JWT trong app (để sau: B4) |
| D9 | Service start type **Automatic** (không Delayed) | Lúc khởi động app không cần mạng, không gọi Google | Delayed auto start |
| D10 | Mỗi request được xếp **Local / Published / Remote** (`infrastructure::ingress`); Published phải mang đúng `Host` trong `PUBLIC_HOST` | Bind loopback không còn nghĩa là "người ngồi tại máy" khi có tunnel; `Host`/`Origin`/`Sec-Fetch-Site` chặn trang web lạ và DNS rebinding | Chỉ dựa vào địa chỉ bind; chỉ dựa vào header của Cloudflare (trang web tự đặt được) |
| D11 | Không có bước nâng quyền (UAC) để dừng service; thiếu quyền thì báo và dùng bản đang chạy | Bước nâng quyền làm ẩn cửa sổ console và khó đúng; installer (S1) sẽ cấp quyền start/stop | `ShellExecuteExW runas` (kéo user32/shell32 vào server.exe), PowerShell `-Verb RunAs` |
| D12 | Bản chạy tay dừng service thì một PowerShell ẩn chờ nó thoát rồi bật lại service | Bật lại được cả khi bản chạy tay bị kill hay đóng cửa sổ | Bật lại trong chính app (đóng cửa sổ thì không kịp chạy) |
| D13 | VPS nhỏ (1 vCPU / 2 GB) chạy Linux, không chạy Windows | Riêng Windows Server có giao diện đã cần 2 GB; app nhàn rỗi chỉ ~17 MB nhưng ghép bản ghi cả đề ước 250–350 MB | VPS Windows 2 GB |

Khi nào xem lại D1: khi cài cho máy/trường khác, hoặc khi Defender/EDR chặn
NSSM (Defender có mục `PUA:Win32/NSSM` từ 2023-08; NSSM không còn được bảo trì
từ 2017, file không được ký). Lúc đó làm B1.

---

## G1 — Sửa app

**Đã làm trong 0.9.0.** Thiết kế cuối cùng nằm trong `docs/architecture.md`
(mục *Request origin* và *Instances and takeover*) và CHANGELOG 0.9.0. Phần
dưới là kế hoạch ban đầu; khác biệt chính sau hai vòng review: thêm luật
`Host`/`Origin`/`Sec-Fetch-Site` cho request cục bộ; `PUBLIC_HOST` bắt buộc đi
cùng `PUBLIC_PORT`; `Cache-Control: private, no-cache`; khoá thư mục dữ liệu
(`instance.lock`) và `instance.json` có stop-token; PID của bản đang chạy phải
đúng là tiến trình đang giữ cổng; không có bước nâng quyền (D11).

### A1. Quy tắc "máy cục bộ" theo từng request, cổng riêng cho tunnel

**Vấn đề.** Nhập API key trong trình duyệt, tạo và xoá giọng thiết kế đều chỉ
được phép khi `local_server()` đúng, mà hàm này chỉ hỏi "server có bind vào
loopback không". `cloudflared` gọi vào `127.0.0.1`, nên mọi người dùng từ
Internet đều được coi là đang ngồi tại máy. Hậu quả: khi key thiếu hoặc bị
Google từ chối, họ nhập được key của họ vào server; và họ tạo hoặc xoá được
giọng trong project Google của bạn (tối đa 200 giọng mỗi project).

**Thiết kế.**

1. **Cấu hình.** Thêm `PUBLIC_PORT` (tuỳ chọn) vào `SETTINGS` trong
   `infrastructure/config.rs`, kiểm tra 1–65535 và khác `PORT`.
   `AppConfig.public_address: Option<SocketAddr>` dùng cùng `IP`.
2. **Khởi động.** Có `public_address` thì bind thêm một listener trong
   `startup::run`. Cùng router, thêm một layer gắn đánh dấu "request đến từ cổng
   công khai" (extension của request) cho mọi request vào cổng này. Hai
   listener dùng chung graceful shutdown. Cổng bận thì báo lỗi cùng kiểu với
   `PORT`. Đường `dioxus::serve` (chỉ khi `dx serve`) không mở cổng thứ hai;
   ghi rõ trong tài liệu.
3. **Quy tắc.** `application::settings::local_server()` đổi thành
   `local_request()`. Request là cục bộ khi **tất cả** đều đúng:
   - server bind vào loopback (như hiện nay);
   - request không mang đánh dấu cổng công khai;
   - request không có header nào trong số `cf-ray`, `cf-connecting-ip`,
     `cf-warp-tag-id`, `x-forwarded-for`, `forwarded`, `x-real-ip`.
     Giả mạo các header này chỉ làm giảm quyền, không tăng quyền.

   Request được đọc qua `FullstackContext::current()` (có trong
   `dioxus-fullstack-core` 0.7.2). Không có context thì coi là **không** cục bộ.
   Phần kiểm tra header là một hàm thuần để test được.
   **Cần xác minh:** extension do layer ngoài gắn vào có tới được `parts` mà
   `FullstackContext` thấy không. Nếu không, dùng một header nội bộ do layer gắn
   vào và xoá mọi header cùng tên mà client gửi lên.
4. **Chỗ phải đổi.** `current_status` / `api_key_status`, `set_api_key`
   (`application/settings.rs`); `design_voice`, `designed_voices`,
   `delete_voice` (`application/voices.rs`). Dòng `key_note` lúc khởi động
   (`infrastructure/mod.rs`) vẫn theo địa chỉ bind, thêm một dòng khi có
   `PUBLIC_PORT`. Các câu thông báo hiện có (`entry_refusal`, `design_refusal`)
   giữ nguyên.

**Test.**

- Mỗi header trong danh sách đều làm request thành "từ xa".
- `PUBLIC_PORT` sai (0, chữ, trùng `PORT`) bị từ chối, thông báo có tên biến.
- Các test hiện có của `entry_refusal` và `design_refusal` vẫn qua.
- Thủ công: gọi `api_key_status` qua 8080 thì `can_enter` như cũ; qua 8081 thì
  `false`; qua 8080 kèm `CF-Connecting-IP: 1.1.1.1` thì `false`.

**Xong khi:** cả ba thao tác (nhập key, tạo giọng, xoá giọng) bị từ chối qua
8081 và qua header, vẫn chạy qua 8080 tại máy; hai `cargo check` và toàn bộ
test xanh.

### A2. Job ghi âm bị bỏ dở khi server khởi động lại

**Vấn đề.** Ghi âm chạy trong task tokio; tắt tiến trình là mất task, còn hàng
trong bảng `jobs` vẫn ở `pending` / `processing`:

- trình duyệt chờ tới hạn 15 phút (part) hay 45 phút (cả đề)
  ([ui/jobs.rs:13](src/ui/jobs.rs:13)) mới bỏ;
- trang Exam mở lại một đề vẫn hỏi trạng thái một job không bao giờ xong;
- các job này vẫn tính vào `MAX_ACTIVE_JOBS = 10`
  ([audio.rs:28](src/application/audio.rs:28)) cho tới khi bị dọn sau
  `AUDIO_RETENTION_HOURS` (không bao giờ nếu là 0).

Có service tự khởi động lại (crash, Windows Update ban đêm) thì chuyện này xảy
ra thường hơn.

**Thiết kế.** Trong `JobStore::initialize`, trước khi nhận request: chuyển mọi
job `pending` / `processing` sang `failed`, kèm lời nhắn giáo viên đọc được, ví
dụ *"The server restarted while this recording was being made. Start the
recording again."*. Job đã xong hoặc đã lỗi giữ nguyên.

**Test.** `#[tokio::test]`: tạo job, đánh dấu processing, mở lại store; job
thành failed với đúng lời nhắn, job completed không đổi, `active_count() == 0`.
Thủ công: ghi âm, dừng service giữa chừng, khởi động lại; trang hiện lỗi ngay
thay vì chờ.

### A3. `Cache-Control` cho audio và mẫu giọng

`/audio/{job_id}` và `/voice-sample/{voice_id}`
([serve.rs:24](src/infrastructure/jobs/serve.rs:24)) không có đuôi file nên
Cloudflare mặc định không cache. Vẫn thêm `Cache-Control: private, no-store`
(cả cho 404) để không phụ thuộc vào mặc định đó. Test: header có mặt; `Range`
vẫn trả 206.

### A4. Theo dõi giới hạn 125 giây

Cloudflare trả **524** nếu server không trả lời trong **125 giây** (tài liệu
cập nhật 2026; bài cũ ghi 100 giây). Chỉ gói Enterprise nâng được, và với
tunnel không có cách tắt proxy. Mỗi lần gọi text có timeout 90 giây, cộng retry
và backoff tới 60 giây ([gemini.rs:36](src/infrastructure/llm/gemini.rs:36)),
nên khi Google bận, một server function có thể vượt ngưỡng: giáo viên thấy lỗi,
server vẫn chạy xong và vẫn tính tiền. Ghi âm không bị, vì đã là job + polling.

Việc ở 0.9.0: chỉ **đo**. Sau P7, theo dõi 2 tuần trong Cloudflare Analytics
(mã 524) và log app. Có 524 thì làm B2.

---

## G2 — Windows service

### S8. Dải cổng động (làm trước khi cài service)

Ngày 2026-10-07 máy này không bind được cổng 8080: dải cổng động TCP đang bắt
đầu từ 1024 (mặc định của Windows là 49152), nên Hyper-V/WinNAT giữ các dải
7610–8109 lúc khởi động, trong đó có 8080 và 8081. Kiểm tra bằng
`netsh interface ipv4 show excludedportrange protocol=tcp` và
`netsh int ipv4 show dynamicport tcp`. Sửa (PowerShell quyền admin, rồi khởi
động lại máy):

```powershell
netsh int ipv4 set dynamic tcp start=49152 num=16384
netsh int ipv6 set dynamic tcp start=49152 num=16384
```

Nếu không muốn đổi, chọn `PORT`/`PUBLIC_PORT` ngoài mọi dải đang bị giữ và để
installer (S1) kiểm tra lại mỗi lần cài.

### Bố trí

| Thứ | Vị trí / giá trị |
|---|---|
| Chương trình | `C:\Program Files\ListeningExamGenerator\server.exe`, `public\`, `RUST-DEPENDENCIES.txt`, lấy từ `target\dx\vmq_mvp\release\web\` do `packaging/build-windows.ps1` build (CRT tĩnh, console subsystem) |
| Cấu hình và dữ liệu | `C:\ProgramData\ListeningExamGenerator\` gồm `.env`, `data\` (`jobs.db`, `audio\`, `logs\`, `voices.json`) |
| Lệnh | `server.exe --config-dir C:\ProgramData\ListeningExamGenerator --service ListeningExamGenerator` (không `--portable`) |
| `.env` | `IP=127.0.0.1`, `PORT=8080`, `PUBLIC_PORT=8081`, `PUBLIC_HOST=app.ielts-ai-master.co.uk`, `DATA_DIR=./data`, `GEMINI_API_KEY=…`, `RUST_LOG=info` |
| Tên service | `ListeningExamGenerator`, hiển thị "Listening Exam Generator" |
| Tài khoản | `NT SERVICE\ListeningExamGenerator` |

### Thiết lập NSSM

| Tham số | Giá trị | Lý do |
|---|---|---|
| `Application` | đường dẫn `server.exe` ở trên | |
| `AppParameters` | `--config-dir C:\ProgramData\ListeningExamGenerator` | |
| `AppDirectory` | `C:\ProgramData\ListeningExamGenerator` | Mặc định NSSM là System32 |
| `ObjectName` | `NT SERVICE\ListeningExamGenerator` | D3 |
| `Start` | `SERVICE_AUTO_START` | D9; chạy khi máy khởi động, không cần đăng nhập |
| `AppExit Default` | `Restart` | Tự chạy lại khi crash |
| `AppRestartDelay` | `5000` | |
| `AppThrottle` | `10000` | Cấu hình sai (exit 1) không thành vòng lặp dồn dập |
| `AppStopMethodConsole` | `4000` | Kịp trong 5 giây `WaitToKillServiceTimeout` của máy này |
| `AppNoConsole` | `0` (mặc định) | Cần console riêng để Ctrl+C tới được app |
| `AppStdout` / `AppStderr` | `…\data\logs\service.log` (cùng một file) | Lỗi khởi động in ra stderr trước khi log của app mở |
| `AppRotateFiles` / `AppRotateOnline` / `AppRotateBytes` | `1` / `1` / `10485760` | stdout chứa mọi dòng log nên phải xoay vòng khi đang chạy. NSSM cảnh báo chế độ online "dễ lỗi hơn"; nếu S2 thấy vấn đề thì cho `AppStdout` ra `NUL`, vì app đã tự ghi log ngày trong `data\logs` |
| `AppEnvironmentExtra` | **không dùng** | D4 |
| Recovery (SCM) | `sc.exe failure ListeningExamGenerator reset= 86400 actions= restart/60000/restart/60000/restart/60000` | Dự phòng khi chính `nssm.exe` chết; app chết thì NSSM đã tự lo |

### S1. `packaging/install-service.ps1`

Tham số: `-Source` (mặc định `target\dx\vmq_mvp\release\web`), `-InstallDir`,
`-ConfigDir`, `-ServiceName`, `-Port 8080`, `-PublicPort 8081`,
`-CopyKeyFromUserEnvironment`, và chế độ `-Upgrade` / `-Uninstall [-RemoveData]`.

Cài mới, theo thứ tự:

1. Bắt buộc chạy với quyền admin. Kiểm tra `nssm version` ≥ 2.24-101; bản 2.24
   gốc thì dừng và nói lý do.
2. Chép chương trình vào `InstallDir`. Giữ ACL mặc định của Program Files:
   Users chỉ đọc, nên không ai thay được `server.exe`.
3. Tạo `ConfigDir`, **gỡ kế thừa ACL** rồi chỉ cho `Administrators` (F),
   `SYSTEM` (F). Gỡ kế thừa trên cả thư mục, không riêng `.env`: "Remember" ghi
   `.env` bằng file tạm rồi đổi tên, file mới mang ACL của thư mục.
4. Tạo `.env` nếu chưa có, **không bao giờ ghi đè**. Với
   `-CopyKeyFromUserEnvironment`, chép `GEMINI_API_KEY` từ HKCU của người đang
   chạy script vào `.env`, không in key ra.
5. `nssm install` và các `nssm set` ở bảng trên.
6. Cấp `NT SERVICE\ListeningExamGenerator` quyền Modify trên `ConfigDir`. Bước
   này phải sau bước 5: tài khoản ảo chỉ tồn tại khi đã có service.
7. `sc.exe failure …`, khởi động service, chờ `http://127.0.0.1:8080/` và
   `:8081/` trả 200 (tối đa 30 giây), in trạng thái và nơi đặt log.

`-Upgrade`: dừng service, chép chương trình mới, giữ nguyên cấu hình và dữ liệu,
khởi động lại. `-Uninstall`: dừng, `nssm remove … confirm`; giữ `ConfigDir` trừ
khi có `-RemoveData`. Chạy lại script nhiều lần cho cùng kết quả.

### S2. `packaging/service-smoke.ps1`

Chạy trên máy có service. Không gọi Gemini, trừ khi có `-Paid`.

| # | Kiểm tra | Mong đợi |
|---|---|---|
| 1 | Trạng thái service | Running, Automatic, đúng tài khoản ảo |
| 2 | `GET /` qua 8080 và 8081 | 200 |
| 3 | `api_key_status` qua 8080 / 8081 / 8080 kèm `CF-Connecting-IP` | nguồn key là `.env`; `can_enter` chỉ có thể `true` qua 8080 không header |
| 4 | `Stop-Service` | `service.log` có "Server stopped cleanly." và dừng xong dưới 4 giây |
| 5 | Kill `server.exe` | NSSM chạy lại trong khoảng 15 giây |
| 6 | Thư mục tạm | Không có `listening-generator-*` mới trong `%TEMP%` của hệ thống |
| 7 | ACL | Users không đọc được `ConfigDir\.env`; tài khoản ảo đọc được |
| 8 | Log | `service.log` xoay vòng khi vượt ngưỡng (chạy với ngưỡng nhỏ) |
| 9 | (`-Paid`, khoảng 0,1 USD) | Ghi âm một part, dừng service giữa chừng, khởi động lại: job thành failed ngay (A2) |

### S3–S7

- **S3** `docs/windows-service.md` (tiếng Anh): cài, nâng cấp, gỡ, đặt key, xem
  log, xử lý sự cố, và lý do của từng thiết lập.
- **S4** Xoá service cũ, với quyền admin: `nssm remove VMQ-MVP confirm`. Trước
  đó xem `nssm dump VMQ-MVP` để biết thư mục dữ liệu và log cũ còn gì cần giữ.
- **S5** Dừng mọi bản app đang chạy (`dx serve`, EXE portable). Chép `jobs.db`
  và `audio\` (và `voices.json` nếu đã sửa) vào `ConfigDir\data\`. Job chỉ lưu
  tên file nên đổi thư mục không mất bản ghi.
- **S6** Build (`packaging/build-windows.ps1`), chạy S1, rồi S2.
- **S7** Ghi kết quả vào `docs/service-verification-v0.9.0.md`, cùng kiểu với
  `docs/portable-verification-v0.8.0.md`.

---

## G3 — Gia cố `cloudflared`

`cloudflared` **không tự cập nhật trên Windows**. Mỗi lần cập nhật hay cài lại,
tunnel đứt vài giây, cả các route khác đang có.

| ID | Cách làm | Kiểm tra |
|---|---|---|
| C1 | `winget upgrade --id Cloudflare.cloudflared -e`, hoặc MSI mới nhất từ GitHub releases của Cloudflare | `cloudflared --version` ≥ 2026.7.3 |
| C2 | Dashboard → Networks → Connectors → tunnel đang dùng → rotate token | Có token mới |
| C3 | Quyền admin: `cloudflared service uninstall`, rồi `cloudflared service install <token mới>` | ImagePath có `--token-file C:\ProgramData\cloudflared\token`, không còn `--token`; dashboard báo connector khỏe; các route khác vẫn chạy |
| C4 | Nếu bản cài chưa đặt: `sc.exe failure cloudflared reset= 86400 actions= restart/20000/restart/20000/restart/60000` | `sc qfailure cloudflared` có ba hành động restart |
| C5 | (Tuỳ chọn) Thêm `--loglevel info --log-directory C:\ProgramData\cloudflared\logs` trước `run` trong ImagePath | Có file log, xoay vòng 1 MB × 5 |

---

## G4 — Mở app qua Tunnel + Access

Đường dẫn trên dashboard theo giao diện sau 2025-11 (Zero Trust đổi tên thành
Cloudflare One; "Public hostname" giờ là "Published application").

| ID | Cách làm |
|---|---|
| P1 | Zero Trust → Integrations → Identity providers → thêm **One-time PIN** (giờ không tự có), và/hoặc Google (cần OAuth client trên GCP) |
| P2 | Access controls → Applications → Add → **Self-hosted**, hostname theo Q1. Policy **Allow**, Include **Emails** theo Q2. Phiên đăng nhập 1 tuần, phiên toàn cục ít nhất bằng thế. Ghi lại AUD tag |
| P3 | Networks → Connectors → tunnel → Routes → Add route → **Published application**: hostname theo Q1, service **`http://127.0.0.1:8081`** (không dùng `localhost`: có thể ra `::1` trong khi app chỉ bind IPv4, gây 502). Bật **Protect with Access** (tên team, AUD tag của P2). DNS CNAME tự tạo |
| P4 | Zone `ielts-ai-master.co.uk`: tắt **Email Address Obfuscation** (bật mặc định, chèn script vào HTML, có thể làm hỏng hydration của Dioxus), **Rocket Loader**, **Bot Fight Mode** (có thể chặn POST của server function và polling); bật **Always Use HTTPS** |
| P5 | Registrar → Manage domain → Configuration → **Enable DNSSEC** |
| P6 | Notifications → **Tunnel Health Alert** gửi về email của PO |
| P7 | Kiểm thử đầu-cuối, bảng dưới |

### P7. Kiểm thử đầu-cuối

Từ điện thoại dùng 4G (không chung mạng với máy chủ), trừ khi ghi khác.

| # | Bước | Mong đợi |
|---|---|---|
| 1 | Mở hostname trong cửa sổ ẩn danh | Trang đăng nhập Access; email ngoài danh sách bị từ chối |
| 2 | Đăng nhập bằng OTP | App tải được, không hiện form nhập key |
| 3 | Mở hộp thiết kế giọng | Nút Design tắt, có lời giải thích; không có nút xoá |
| 4 | Sinh một part (script + câu hỏi + ghi âm) | Xong; ghi lại thời gian từng bước so với ngưỡng 125 giây (A4) |
| 5 | Nghe, tua giữa file WAV, tải WAV và DOCX | Tua được (`Range` qua Cloudflare: **cần xác minh**), tải đủ dung lượng |
| 6 | Trang Exam: lưu, tải lại, mở lại | Đề còn nguyên |
| 7 | Tại máy chủ: `http://127.0.0.1:8080` | Thiết kế giọng được (cục bộ) |
| 8 | Tại máy chủ: `curl` vào 8080 kèm `CF-Connecting-IP` | Bị coi là từ xa |
| 9 | Tạm đặt phiên Access 15 phút, chờ hết hạn rồi bấm sinh | Ghi lại thông báo lỗi thực tế. Phiên hết hạn làm POST bị chuyển sang `cloudflareaccess.com` và lỗi CORS (B5) |
| 10 | Đo thời gian tải một file WAV cả đề (khoảng 87 MB) | Ghi vào CHANGELOG; nếu chậm thì nâng ưu tiên MP3 (B3) |

---

## G5 — Máy luôn bật

| ID | Việc | Ghi chú |
|---|---|---|
| M1 | BIOS/UEFI: **Restore on AC Power Loss / AC Recovery / After Power Loss = Power On** | Tên mục tuỳ hãng (Q6). Chỉ có tác dụng khi **mất điện thật**: bấm Shut down bình thường thì máy không tự bật |
| M2 | Settings → Windows Update → Advanced → **Active hours** trùng giờ dạy | Ngoài giờ đó Windows có thể tự restart; service tự lên lại, không cần đăng nhập |
| M3 | Restart máy, **không đăng nhập** | Từ điện thoại vào được app; ghi lại thời gian từ lúc bấm Restart tới lúc trang mở |
| M4 | Rút điện, cắm lại | Máy tự bật (M1), app vào được từ ngoài |
| M5 | Cài đặt nguồn | ✅ Đã đúng: Fast Startup tắt, sleep/hibernate khi cắm điện = never |

---

## G6 — Phát hành 0.9.0

**Đã phát hành 2026-10-07** với phần sửa app (G1); xem CHANGELOG và
`docs/portable-verification-v0.9.0.md`. Các bảng số về service, tunnel và tải
WAV qua mạng (R2 ban đầu) chuyển sang bản có installer.

- **R1 Tài liệu:**
  - `.env.example` và bảng cấu hình trong README: thêm `PUBLIC_PORT`.
  - README: mục "Run as a Windows service" và "Publish with Cloudflare Tunnel".
  - CLAUDE.md và AGENTS.md: quy tắc "loopback bind" đổi thành "request từ chính
    máy này"; thêm `PUBLIC_PORT` và việc job bị bỏ dở thành failed lúc khởi động.
  - `docs/architecture.md`: mục Deployment shape; ghi tiến độ mục 2 của lộ
    trình sản phẩm.
  - `docs/portable.md`: câu về loopback.
- **R2** CHANGELOG 0.9.0 (tiếng Việt) với bảng "Con số biết nói": thời gian
  khởi động sau Restart, thời gian dừng service, thời gian từng bước khi đi qua
  tunnel, tốc độ tải WAV.
- **R3** `cargo fmt --check`, `cargo check`,
  `cargo check --features server --no-default-features`,
  `cargo test --features server --no-default-features`, build và
  `packaging/smoke.py` cho EXE portable (bản portable không đổi hành vi).
- **R4** `version = "0.9.0"` trong `Cargo.toml`.
- **R5** Commit / tag khi PO đồng ý.

---

## Để sau (backlog)

| ID | Việc | Khi nào làm |
|---|---|---|
| B1 | `server.exe` tự chạy được như service bằng crate `windows-service` (Mullvad, MIT/Apache, 0.8.1): thêm `service install / uninstall`, dừng êm khi nhận Stop/Shutdown, bỏ phụ thuộc NSSM | Cài cho máy khác, hoặc Defender/EDR chặn NSSM |
| B2 | Sinh script và câu hỏi thành job + polling như ghi âm | Có lỗi 524 (A4) |
| B3 | Xuất MP3 (mục 1 lộ trình sản phẩm): đề 30 phút từ khoảng 87 MB xuống khoảng 14 MB ở 64 kbps mono | Tải WAV qua mạng nhà chậm (P7-10) |
| B4 | Kiểm `Cf-Access-Jwt-Assertion` trong app (khoá ở `https://<team>.cloudflareaccess.com/cdn-cgi/access/certs`, kiểm `aud`, `iss`, `exp`), biết email người dùng: giới hạn theo người, đề của ai người nấy thấy | Có nhiều giáo viên dùng chung |
| B5 | UI nhận ra phiên Access hết hạn và bảo giáo viên tải lại trang | Giáo viên gặp lỗi khó hiểu sau P7-9 |
| B6 | Cho một số email (admin, qua B4) được Voice Design từ xa | Q4 trả lời "có" |
| B7 | Sao lưu hằng ngày `jobs.db` + `audio\` sang ổ khác hoặc đám mây (Task Scheduler + `sqlite3 .backup` hoặc dừng ngắn) | Ngay khi có đề quan trọng trên máy chủ: chỉ có một máy, hỏng ổ là mất hết |
| B8 | Chế độ dev và prod tách bạch | PO nêu 2026-10-07 |
| B9 | Ghép bản ghi cả đề thẳng ra file thay vì giữ cả trong RAM (đỉnh ước 250–350 MB mỗi bản ghi đề, `MAX_ACTIVE_JOBS = 10`) | Trước khi chạy trên VPS 2 GB |
| B10 | Chạy `wasm-opt` trong container Linux (WASM 4,1 MB chưa tối ưu vì `wasm-opt` crash trên Windows) | Khi giáo viên mở lần đầu qua 4G thấy chậm |
| B11 | Đo thật trên container 1 vCPU / 2 GB: RAM đỉnh và thời gian khi ghi một đề đầy đủ (khoảng 0,36 USD) | Chờ PO đồng ý |
| B12 | Service tự bật lại cả khi người dùng đăng xuất sau khi đã dừng nó (watcher hiện là PowerShell trong phiên người dùng) | Nếu chuyện này xảy ra thật |

---

## Rủi ro

| Rủi ro | Khả năng | Ảnh hưởng | Giảm thiểu |
|---|---|---|---|
| Defender/EDR chặn `nssm.exe` (`PUA:Win32/NSSM`) | Thấp trên máy này (đã chạy được) | Service không chạy | B1; tạm thời thêm ngoại lệ cho đúng đường dẫn |
| Mở route trước khi xong A1 hoặc P2 | Trung bình | Người lạ dùng tiền Gemini, nhập key, xoá giọng | Thứ tự G1 → G2 → P2 → P3; P3 phụ thuộc A1 trong bảng tracking |
| Lỗi 524 khi Google chậm | Thấp–trung bình | Mất kết quả một bước nhưng vẫn mất tiền | A4, B2 |
| Token tunnel lưu kém an toàn ở bản `cloudflared` cũ | Trung bình | Ai có token dựng được connector giả cho tunnel | C1–C3 (cập nhật, rotate, cài lại) |
| Đường upload của mạng nhà chậm | Trung bình | Tải WAV lâu | B3 |
| Ổ đĩa đầy do audio | Thấp | Ghi âm lỗi | `AUDIO_RETENTION_HOURS`, `SPEECH_CACHE_HOURS`; theo dõi dung lượng `data\audio` |
| Hỏng máy hoặc ổ | Thấp | Mất đề đã lưu | B7 |
| Giới hạn Zero Trust Free (nghe nói 50 người, **chưa xác minh** trên tài liệu chính thức) | Thấp | Không thêm được người | Kiểm tra trước khi mời quá 40 người |
| Windows giữ đúng cổng của app sau khi khởi động lại (dải cổng động từ 1024) | Đã xảy ra trên máy này | Service không bind được, NSSM thử lại mãi | S8 |
| Người qua tunnel tạo giọng tốn tiền và chiếm chỗ trong 200 giọng của project | Thấp (đã có Access) | Tiền và chỗ | `Bucket::VoiceDesign` 5/phút; chỉ máy chủ xoá được |
| Bản 0.8.x dùng chung thư mục dữ liệu với 0.9 | Thấp | 0.9 đánh dấu failed bản ghi đang làm của 0.8 | Tắt bản cũ trước khi chạy 0.9 trên cùng thư mục (ghi trong CHANGELOG) |

---

## Nhật ký

| Ngày | Việc | Kết quả |
|---|---|---|
| 2026-10-05 | Khảo sát máy (NSSM, `cloudflared`, service cũ, nguồn điện) và code (khởi động, cấu hình, quy tắc loopback, job ghi âm); tra tài liệu NSSM, Cloudflare Tunnel và Access | Ra kế hoạch này; tìm ra A1 (bắt buộc), A2, cần cập nhật `cloudflared` (C1–C3) |
| 2026-10-06 | PO trả lời Q1–Q6; phân tích chạy trên VPS 1 vCPU / 2 GB (app nhàn rỗi 17 MB RAM, 0,7 ms CPU mỗi trang) | Hostname `app.`, chỉ Access, bắt đầu trống; thêm N1, N2, V1; VPS nhỏ nên chạy Linux (D13, B9–B11) |
| 2026-10-07 | Thiết kế bước 1, 4 agent phản biện; cài đặt 6 phần, mỗi phần có agent soát riêng; review cuối 6 góc nhìn × 3 phiếu (11 lỗi xác nhận, đã sửa); smoke Windows và Linux, takeover thật trên EXE portable, `dx serve`; phát hiện dải cổng động (S8) | Phát hành 0.9.0 (G1) |

---

## Tham khảo

- NSSM: <https://nssm.cc/download>, <https://nssm.cc/usage>, <https://nssm.cc/bugs>
- Defender PUA:Win32/NSSM: <https://www.microsoft.com/en-us/wdsi/threats/malware-encyclopedia-description?Name=PUA%3AWin32%2FNSSM&threatId=403818>
- Service recovery: <https://learn.microsoft.com/en-us/windows/win32/api/winsvc/ns-winsvc-service_failure_actions_flag>
- Windows Server 2025: <https://learn.microsoft.com/en-us/lifecycle/products/windows-server-2025>
- Crate `windows-service`: <https://github.com/mullvad/windows-service-rs>
- Cloudflare Tunnel: <https://developers.cloudflare.com/tunnel/get-started/>, <https://developers.cloudflare.com/tunnel/reference/origin-parameters/>, <https://developers.cloudflare.com/tunnel/downloads/>, <https://developers.cloudflare.com/tunnel/reference/tunnel-tokens/>
- Lỗi 524 (125 giây): <https://developers.cloudflare.com/support/troubleshooting/http-status-codes/cloudflare-5xx-errors/error-524/>
- Access, ứng dụng self-hosted: <https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/self-hosted-public-app/>
- Phiên Access: <https://developers.cloudflare.com/cloudflare-one/access-controls/access-settings/session-management/>
- Kiểm JWT của Access: <https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/authorization-cookie/validating-json/>
- Header Cloudflare thêm vào: <https://developers.cloudflare.com/fundamentals/reference/http-headers/>
- Cache mặc định: <https://developers.cloudflare.com/cache/concepts/default-cache-behavior/>
- DNSSEC cho Registrar: <https://developers.cloudflare.com/registrar/get-started/enable-dnssec/>
