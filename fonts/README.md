# OpenCADStudio Community Fonts

Thư mục này là **kho font cộng đồng** của OpenCADStudio. Khi mở một bản vẽ
DWG/DXF tham chiếu font (`.shx`) không có trên máy, ứng dụng sẽ hỏi người dùng
và tự động tải font từ thư mục này, thay vì vẽ chữ bằng font thay thế bị lệch/vỡ.

When a drawing references an `.shx` font that is missing on the machine,
OpenCADStudio offers to download it from this folder automatically instead of
rendering with a mismatched substitute font.

## Cách đóng góp / How to contribute

1. Fork repository, thêm font vào thư mục `fonts/` ở **thư mục gốc của repo**
   (cùng cấp với `src/`, `crates/`, …), rồi mở Pull Request.
2. Giữ nguyên **tên file viết thường** như bản vẽ tham chiếu, ví dụ
   `vnarial.shx`, `romans.shx`, `simplex.shx`. Ứng dụng so khớp tên file
   không phân biệt hoa/thường.
3. Định dạng hỗ trợ: `.shx` (compiled shape font).
4. **Giấy phép / License**: chỉ đóng góp font bạn có quyền phân phối lại —
   font do bạn tự vẽ, font nguồn mở, hoặc font thuộc public domain.
   Ghi nguồn + giấy phép vào cuối file này theo mẫu bên dưới.
   Only contribute fonts you have the right to redistribute, and record the
   source and license below.

Ứng dụng tải font về thư mục cài đặt người dùng
(`%APPDATA%/OpenCADStudio/fonts` trên Windows) và dùng lại cho các lần mở sau.

## Danh mục font / Font catalog

| File | Nguồn / Source | Giấy phép / License |
|------|----------------|---------------------|
| —    | —              | —                   |
