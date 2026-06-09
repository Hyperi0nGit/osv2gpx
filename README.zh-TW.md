# osv2gpx

`osv2gpx` 是給 DJI Avata 360 錄製的 OSV 檔使用的工具，用來準備 Google
Street View 上傳所需的 GPX 軌跡、MP4 時間資訊與 JPG GPS metadata。

用途是：

1. 從 OSV 檔取出 GPS 資訊，存成 GPX 檔。
2. 用 GPX 檔中的資訊，幫 DJI Studio 匯出的 MP4 檔加上時間資訊。
3. 先用 ffmpeg 從 MP4 檔轉成每秒一張 JPG，再用 GPX 檔中的資訊幫每一張
   JPG 加上 GPS 資訊。

[English](README.md)

## 使用方式

### 1. 從 OSV 產生 GPX

從原始 `flight.OSV` 產生 `flight.gpx`：

```powershell
osv2gpx flight.OSV
```

在 Windows 上，也可以直接將 OSV 檔拖曳到 `osv2gpx.exe`。GPX 檔會產生在
OSV 檔所在的同一個目錄。

為每個 OSV 輸入各產生一個 GPX 檔：

```powershell
osv2gpx flight1.OSV flight2.OSV flight3.OSV
```

### 2. 幫 DJI Studio 匯出的 MP4 加上時間資訊

將 GPX 第一個時間寫入 DJI Studio 匯出的 MP4：

```powershell
osv2gpx flight.mp4 flight.gpx
```

### 3. 幫 JPG 加上 GPS 資訊

從匯出的 MP4 每秒產生一張 JPG，並將 GPS EXIF 與 GPano XMP 寫入這些 JPG：

```powershell
mkdir jpg-dir
ffmpeg -i flight.mp4 -vf fps=1 -q:v 2 jpg-dir\frame_%06d.jpg
osv2gpx jpg-dir flight.gpx
```

`osv2gpx` 會依檔名排序處理產生的 JPG。第一張使用 GPX 第一個時間，第二張
使用一秒後的時間，依此類推；GPS 位置會從 GPX 軌跡插值取得。GPano XMP 會
使用圖片實際寬高，將每張 JPG 標記為完整 equirectangular 全景圖。
產生的 JPG 可透過 Google Street View Publish API 上傳，例如使用
[stviewpub](https://znbang.github.io/stviewpub/)
（[專案](https://github.com/znbang/stviewpub)）。

## 輸出

GPX 會包含一個 track segment，內含多個 `trkpt`：

```xml
<trkpt lat="24.79920562" lon="121.05540174">
  <ele>201.138</ele>
  <time>2026-05-27T09:23:16.647Z</time>
</trkpt>
```

高度使用 absolute altitude，單位為 meters。

## 編譯

先安裝 stable [Rust toolchain](https://rustup.rs/)，再編譯 release 執行檔：

```powershell
cargo build --release --locked
```

## 注意事項

- 請使用原始 OSV 檔產生 GPX 軌跡。DJI Studio 匯出的 MP4 不會保留 DJI 的
  GPS metadata tracks。
- DJI Studio 匯出的 MP4 也沒有 creation time metadata。若已經有對應的 GPX
  檔，`osv2gpx` 可以將 GPX 第一個時間寫入 MP4 creation time 欄位。
- GPX 檔包含 latitude、longitude、absolute altitude 與 timestamp。JPG 的
  GPS 位置會從 GPX 軌跡插值取得。
- MP4 creation time 欄位只有秒級精度，而 DJI SRT 可能包含毫秒級時間。
