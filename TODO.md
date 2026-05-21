# CẨM NANG HÀNH XÁC: ĐỘ THÀNH MVP "RA HỒN"

Dành cho thợ code Rust đang rảnh rỗi sinh nông nổi, muốn đấm nhau với Borrow Checker xuyên đêm. Dưới đây là list các nghiệp chướng cần độ hóa trước khi xách con app này đi deploy lên con máy Win 10 cỏ Port 80.

## 1. Dẹp ngay cái trò lưu Job Queue và Audio File vào RAM (In-memory)
**Độ ngu:** 10/10. Máy Win 10 cỏ mở app lên một lúc, người ta render 10 bài nghe mỗi bài 30MB là tràn RAM, app crash bay sạch data, về máng lợn.
- [x] Tích hợp `rusqlite` (SQLite cho Rust) để lưu thông tin và trạng thái (Status, Job ID, File Path, Created_At) của các bài luyện nghe đang được sinh. *(Đã hoàn thành! Job metadata đã được lưu bằng SQLite ở E:\vmq_data\audio_jobs.db)*
- [x] Thay vì ném mảng `Vec<u8>` vào `Arc<Mutex<HashMap>>`, hãy ghi thẳng byte array nhận được từ API thành tệp `.wav` xuống ổ cứng (ví dụ: `E:\vmq_data\audio\`). Database chỉ lưu đường dẫn `file_path`. *(Đã hoàn thành! File lưu ở `E:\vmq_data\audio\`)*
- [x] Viết một background task chạy bằng Tokio (chạy timer mỗi giờ) để quét dọn xóa rác các file audio đã quá 24h để máy cỏ không bị đầy cạn ổ cứng. *(Đã hoàn thành! Đã có hàm `start_cleanup_task`)*

## 2. API Error Handling: Không được Panic & Không "Nuốt Lỗi"
**Độ ngu:** 8/10. Dùng `reqwest`, mạng chập chờn hay Gemini dở chứng là văng `unwrap_or_default()` móm luôn, chả có cơ chế hồi phục.
- [x] Triển khai cơ chế **Exponential Backoff Retry** (thử lại có thời gian chờ tăng dần) khi gọi Gemini API. Bắt và xử lý triệt để các mã lỗi 429 (Too Many Requests) và 503 (Service Unavailable). *(Đã hoàn thành! Đã tích hợp vòng lặp exponential backoff bằng tokio::time::sleep)*
- [x] Thêm các Timeout chuẩn chỉ thay vì để request treo vô cực. Trả về thông báo lỗi "Server đang tải nặng, vui lòng thử lại sau" ra Frontend đàng hoàng thay vì văng đống text lỗi khó hiểu. *(Đã hoàn thành! Đã xử lý timeout và trả về thông báo lỗi chuẩn tiếng Anh ra Frontend)*

## 3. Quản lý Voice Config (Chống Hardcode)
**Độ ngu:** 7/10. Hardcode mấy giọng "Puck", "Kore", "Aoede"... dính chặt vào trong code.
- [x] Tách toàn bộ phần mapping giọng đọc (Giới tính + Vùng miền + Role -> Voice Name) ra một tệp cấu hình bên ngoài `voices.json`. *(Đã hoàn thành! Cấu hình được lưu ngoài ở E:\vmq_data\voices.json)*
- [x] Dùng `serde_json` và `once_cell` (hoặc `lazy_static`) để load file này vào RAM lúc khởi động server. Đời thay đổi, AI ra giọng mới thì chỉ việc cập nhật file JSON chứ cấm được lôi code ra compile lại! *(Đã hoàn thành! Thay vì load 1 lần, hệ thống đọc file json linh hoạt mỗi lần sinh audio để thay giọng không cần restart app)*

## 4. Frontend & Dioxus Signal Chaos
**Độ ngu:** 6/10. File `home.rs` như một cái nồi lẩu thập cẩm các loại `use_signal`.
- [x] Refactor (cấu trúc lại) `home.rs`. Gom đống state lộn xộn của form thành một Struct xịn xò và gọi 1 phát `use_signal(|| FormState::default())`. *(Đã hoàn thành! Đã gom thành HomeState)*
- [x] Component hóa: Tách cái Modal chỉnh sửa Speaker và cục Audio Player ra các file component riêng biệt cho dễ bảo trì. *(Đã hoàn thành! Tách ra folder src/components/)*

## 5. Deployment Setup (Dành cho Win 10 Cỏ - Port 80)
**Độ ngu:** Mở port 80 cho máy cá nhân dễ oẳng, code thì chạy mặc định localhost, sập thì nghỉ chơi.
- [x] **Config Port 80**: Vào file cấu hình khởi chạy Dioxus, bind IP trực tiếp về `0.0.0.0` và cổng `80` (thay vì để mặc định localhost port 8080). *(Đã làm: Nhưng thông qua `std::env::set_var` thay vì struct `Config`)*
  ```rust
  // Đâu đó trong main.rs
  let config = dioxus::fullstack::Config::new().addr(std::net::SocketAddr::from(([0, 0, 0, 0], 80)));
  dioxus::launch_with_props(App, (), config);
  ```
- [x] **Mở Cửa Sổ (Windows Firewall)**: Viết một file script `setup.bat` nhỏ chứa lệnh xé rào tường lửa: 
  `netsh advfirewall firewall add rule name="VMQ MVP Port 80" dir=in action=allow protocol=TCP localport=80` *(Đã làm: `setup.bat` đã mở cổng 80 và 443 đầy đủ)*
- [x] **Treo App Xuyên Đêm (Daemonize)**: Máy cỏ Win 10 hay update ngu rồi tự reset. Tải **NSSM** (Non-Sucking Service Manager) về, cài file thực thi `vmq_mvp.exe` (đã build `--release`) thành một Windows Service tự động khởi chạy (Auto-start) cùng hệ thống và tự động Restart nếu app crash. *(Đã làm: App đang được NSSM daemonize dưới nền)*
- [x] **Lưu Log Điều Tra Lỗi**: Tích hợp crate `tracing-appender` ghi log chi tiết ra tệp tin (Rolling log theo ngày). Nếu app chết cứng giữa đêm thì sáng ra còn biết đường chui vào xem máy bị làm sao, chứ in ra stdout của console tắt đi là khóc! *(Đã hoàn thành! Log được lưu daily tại E:\vmq_data\logs\)*

---
**CHỐT LẠI**: 
Đêm nay pha một bình cà phê đen thật đặc, khoá cửa phòng, mở nhạc Doom Slayer lên và ôm lấy con laptop mà sửa cho xong đống nợ nghiệp này. 
Sửa xong thì cái mớ hỗn độn này mới tạm coi là một "MVP ra hồn". Chúc thợ code bình an vô sự bước qua ải Borrow Checker!
