# VMQ MVP: Ứng Dụng Sinh Bài Nghe IELTS (Bản Độ "Chống Cháy")

Chào mừng đến với VMQ MVP - một con app Dioxus Fullstack (Rust) chuyên dùng để tự động sinh đề thi IELTS Listening bằng sức mạnh của Google Gemini API. 

Nếu bạn đang đọc cái này, tức là bạn chuẩn bị đối mặt với Borrow Checker hoặc đang thắc mắc tại sao con máy cỏ Win 10 của mình lại gánh được cái hệ thống này mà không bốc khói.

## Tính Năng "Ra Hồn" (Sau khi đã độ lại)

Hồi xưa con app này lưu file vào RAM, mỗi lần văng là đi tong. Bây giờ thì nó đã thành một MVP thực thụ, đủ sức sống sót qua đêm:

- **Lưu trữ bằng SQLite (`rusqlite`)**: Mọi metadata, trạng thái, Job ID của bài nghe đều được nhét gọn vào `E:\vmq_data\audio_jobs.db`. Rút điện máy tính cắm lại vẫn không mất data!
- **Chống Dội Bom API (Exponential Backoff)**: Chấp Gemini dở chứng báo lỗi 429 hay 503, hệ thống sẽ tự động chờ và thử lại (retries) đàng hoàng thay vì crash tung tóe như trước.
- **Thay Giọng Đọc Nóng (Hot-swap)**: Không còn hardcode mấy giọng "Puck", "Zephyr" trong code nữa. Bạn có thể mở file `E:\vmq_data\voices.json`, đổi tên giọng, lưu lại và hệ thống sẽ ăn ngay lập tức mà không cần khởi động lại app.
- **Xóa Rác Tự Động**: Tích hợp luồng chạy ngầm (background task) để tự động dọn các file audio (`.wav`) cũ hơn 24h. Cứu rỗi cái ổ cứng bé tí của máy Win 10 cỏ.
- **Log Điều Tra Án Mạng**: App tự động lưu Rolling Logs theo ngày tại `E:\vmq_data\logs\vmq-mvp.log.*`. App có chết cứng giữa đêm thì sáng ra mở log lên vẫn biết đứa nào ám hại mình.
- **Frontend Xịn Xò**: 19 cái `use_signal` lộn xộn đã bị tiễn vong. State giờ được quản lý tập trung và Component hóa gọn gàng để tránh render lại cả một cục to đùng khi chỉ cần bấm Play/Pause nhạc.

## Hướng Dẫn Cài Đặt (Dành cho máy Windows 10 Cỏ)

Dưới đây là cẩm nang để vứt con app này lên máy cá nhân (Port 80) chạy 24/7.

### 1. Build Tối Ưu (Release Mode)
Đừng có chạy `cargo run` nữa, build bản release để tối ưu hiệu năng:
```powershell
cargo build --release --features server
```

### 2. Mở Cửa Sổ Tường Lửa (Firewall)
Chạy file script `setup.bat` (Run as Administrator) để đục lỗ Port 80 và 443, cho phép thế giới bên ngoài truy cập vào máy bạn.

### 3. Treo App Xuyên Đêm (Daemonize)
Tải `NSSM` (Non-Sucking Service Manager) về, trỏ thẳng vào cái file `.exe` vừa build xong (thường nằm ở `target/dx/vmq_mvp/release/web/server.exe` nếu build bằng dx, hoặc `target/release/vmq_mvp.exe`):
```powershell
.\nssm.exe install "VMQ-MVP" "E:\vmq_mvp 01-01-2026\target\dx\vmq_mvp\release\web\server.exe"
.\nssm.exe start "VMQ-MVP"
```
Giờ thì app của bạn đã chính thức hóa kiếp thành một Windows Service chạy ngầm. Dù máy bị dở chứng tự restart thì nó vẫn sẽ tự động sống lại.

### 4. Thư Mục Dữ Liệu
App sẽ tự động tạo và sử dụng ổ `E:` để làm nhà của nó (không được xóa nha) (chúa tể works on my machine):
- `E:\vmq_data\audio_jobs.db` -> File CSDL SQLite.
- `E:\vmq_data\audio\` -> Thư mục chứa các file bài nghe `.wav` siêu to khổng lồ.
- `E:\vmq_data\voices.json` -> Nơi bạn vào để chỉnh chọt cấu hình giọng đọc của Gemini.
- `E:\vmq_data\logs\` -> Nghĩa trang chứa log file.

---

**LỜI KẾT**: 
Code đã viết, nghiệp đã độ. Nếu bạn cần bảo trì hay sửa đổi con app này trong tương lai, hãy pha một cốc cà phê đen thật đặc, bật skibidi toilet lên và chuẩn bị sẵn tinh thần. Chúc bình an vô sự bước qua ải Borrow Checker!
