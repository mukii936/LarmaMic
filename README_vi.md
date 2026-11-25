<div align="center">

  <h1>LarmaMic</h1>

  <img src="./img/app_icon.png" width="128" height="128" />

  <br>

  <a href="./README.md">English</a> | <b>Tiếng Việt</b>

  <br>

  <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-000000?style=flat&logo=rust&logoColor=white">
  <img alt="React" src="https://img.shields.io/badge/React-19-61DAFB?style=flat&logo=react&logoColor=black">
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?style=flat&logo=windows&logoColor=white">
  <img alt="Languages" src="https://img.shields.io/badge/languages-10-38e0c8?style=flat">
  <img alt="Version" src="https://img.shields.io/badge/version-0.3.0-946ce6?style=flat">

  <br><br>

  LarmaMic biến iPhone thành micro cho máy tính, mọi việc chỉnh âm đều làm trên PC.

  Một app Windows nhỏ · iPhone chỉ cần Safari · không phải cài gì lên điện thoại

</div>

## Tính năng

- Truyền tiếng từ iPhone qua Wi-Fi: quét mã QR trong app trên PC rồi mở trang bằng Safari. Không cài app nào lên điện thoại.
- Chỉ một file `larma-mic.exe` chạy ngay (khoảng 9 MB, không trình cài, không cần runtime). Khi thu vào khay hệ thống, app dùng ≈ 0% CPU và khoảng 4 MB RAM.
- Xử lý giọng ở 48 kHz: khử ồn AI ([RNNoise](https://jmvalin.ca/demo/rnnoise/)), cắt tiếng ù, cổng ồn nhìn trước 5 ms, EQ 4 dải, tự cân âm lượng (AGC), nén giọng, âm lượng và bộ chống vỡ tiếng −3 dB.
- Hiệu ứng giọng: đổi tông, vang, giọng robot.
- Theo dõi trực tiếp: đồng hồ mức âm, phổ âm đầu ra, đường cong EQ, độ trễ ước tính, bộ đệm, số lần giật và gói bị bỏ.
- Tự đặt ngưỡng cổng ồn: giữ im lặng 3 giây, app đo nền ồn rồi đặt ngưỡng cho bạn.
- Đồng bộ hai chiều: mọi điều khiển có cả trên PC **và** trên trang điện thoại, hai bên luôn giống nhau. Cấu hình nhanh (Giọng nói, Game, Podcast, Gốc) và hồ sơ riêng của bạn dùng được ở cả hai.
- Tắt/bật tiếng bằng phím tắt toàn cục (mặc định `Ctrl + Alt + M`, dùng được trong game) hoặc nút giữ để nói trên điện thoại, có tiếng bíp báo.
- Xuất âm thanh qua VB-Cable hoặc Steam Streaming Microphone để dùng trong Discord, OBS, game, cuộc gọi và app ghi âm.
- Ghi giọng đã lọc ra file WAV chỉ bằng một cú bấm.
- Tắt màn hình vẫn chạy tiếp: khi iOS tạm dừng trang, mic và kết nối tự bật lại ngay khi bạn quay về.
- 10 ngôn ngữ giao diện cho cả app PC và trang điện thoại, tự nhận theo máy.
- An toàn: HTTPS với chứng chỉ tự ký, mã ghép nối ngẫu nhiên trong địa chỉ QR, mỗi lần một điện thoại, cài đặt ghi qua file tạm và giữ bản sao lưu `.bak`.

## Ảnh chụp

### App trên PC

<img src="img/desktop.png" width="900" />

### Trang trên iPhone

<img src="img/phone.jpg" width="300" />

## Bắt đầu

1. Tải `larma-mic.exe` từ [GitHub Releases](https://github.com/mukii936/LarmaMic/releases), hoặc [tự build](#build-từ-mã-nguồn).
2. Chuẩn bị mic ảo (làm một lần):

   | Cách | Làm thế nào |
   |:---|:---|
   | **VB-Cable** | Cài [VB-Cable](https://vb-audio.com/Cable/) (miễn phí). Trong app gọi, game hay phần mềm stream, chọn **CABLE Output** làm micro. |
   | **Steam Streaming Microphone** | Máy đã cài Steam thì không cần cài thêm. Chọn **Steam Streaming Microphone** làm micro. |

   LarmaMic tự chọn một loại (ưu tiên VB-Cable) và cảnh báo nếu đầu ra đang trỏ vào loa thường.
3. Mở LarmaMic, để iPhone và PC cùng Wi-Fi, rồi quét mã QR bằng camera iPhone.
4. Lần đầu Safari báo “không riêng tư” vì chứng chỉ tự ký: chọn **Hiện chi tiết**, rồi truy cập trang web. Nếu điện thoại không vào được, cho phép LarmaMic qua Windows Firewall.
5. Bấm **Bật mic** trên điện thoại và cho phép dùng micro. Bộ chỉnh hiện ra khi đã nối với PC.

Bạn cũng có thể cài vào máy bằng nút **Cài vào máy…** (lối tắt Start Menu và mục trong Settings > Apps, không cần quyền quản trị), hoặc từ dòng lệnh:

```powershell
larma-mic.exe --install      # hoặc --uninstall; thêm --quiet để bỏ hộp thoại
```

## Ngôn ngữ

Tiếng Việt, English, 中文 (简体), 日本語, 한국어, Bahasa Indonesia, Español, Français, Deutsch, Русский.

App PC mặc định theo ngôn ngữ Windows, đổi được ở ô chọn góc trên bên trái; menu ở khay đổi theo. Trang điện thoại theo ngôn ngữ trên PC hoặc theo lựa chọn của bạn. Font CJK chỉ được nạp từ Windows khi chọn các ngôn ngữ đó, để tiết kiệm RAM.

> Các bản dịch do AI viết, chưa có người bản ngữ kiểm tra. Rất mong được sửa giúp.

Thêm ngôn ngữ: tạo `src/lang/xx.rs` (đủ mọi khóa như bảng tiếng Việt trong `src/i18n.rs`) và `web/src/i18n/xx.ts`, rồi đăng ký. `cargo test` báo khóa thiếu/thừa, chỗ giữ chỗ `{0}` lệch và chuỗi tiếng Việt viết cứng; `npm run build` báo thiếu khóa ở trang điện thoại.

## Build từ mã nguồn

Cần Rust (bộ công cụ MSVC) và Node.js.

```powershell
.\build.ps1                 # build trang React rồi build exe: target\release\larma-mic.exe
cargo test --release        # DSP, hiệu ứng, phổ, WAV, cài đặt, khởi động cùng Windows, kiểm tra i18n
```

Chạy một bản thử cạnh bản đang dùng mà không đụng cài đặt thật:

```powershell
$env:APPDATA = "$env:TEMP\larma-test"; $env:LARMA_LOCK_PORT = "47999"; $env:LARMA_PORT = "9443"
.\target\release\larma-mic.exe
```

Riêng trang điện thoại: chạy `npm run dev` trong `web/` để xem giao diện mà không cần nối với PC.

## Công nghệ

- Desktop: Rust, eframe/egui, cpal (WASAPI), axum + rustls, tray-icon, global-hotkey
- Âm thanh: nnnoiseless (RNNoise), chuỗi DSP tự viết, phổ FFT, bộ đệm chống giật
- Trang điện thoại: React, TypeScript, Vite, Web Audio (AudioWorklet), WebSocket
- Giao thức: PCM 16-bit thô qua WebSocket, kèm các tin JSON nhỏ cho thông số và số đo

## Kế hoạch

- Ký số cho file exe
- Chưa hỗ trợ Android và nền tảng khác; trang điện thoại có thể chạy ở trình duyệt di động khác nhưng mới thử trên Safari của iPhone

## Đóng góp

Báo lỗi, sửa bản dịch và pull request đều được chào đón. Với thay đổi lớn, hãy mở issue trước.

## Lời cảm ơn

Cảm ơn [MicYou](https://github.com/MicYou-Dev/MicYou) đã truyền cảm hứng, trong đó có ý tưởng dùng Steam Streaming Microphone làm mic ảo dựng sẵn trên Windows.

Cảm ơn [Jean-Marc Valin](https://github.com/xiph/rnnoise) với RNNoise, [nnnoiseless](https://github.com/jneem/nnnoiseless) với bản Rust, và [VB-Audio](https://vb-audio.com/Cable/) với VB-Cable.
