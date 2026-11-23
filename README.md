# Yarın Yaparım

Projeler için hafif, koyu temalı bir masaüstü yapılacaklar listesi. Rust + [Slint](https://slint.dev) ile yazıldı; Windows'ta tray'de yaşar.

## Özellikler
- Projeler (renkli), görevler, alt görevler, öncelik, sürükle-bırak sıralama
- Deadline'lı veya deadline'sız görev; Türkçe hızlı giriş:
  `FLIR testi cuma 14:00 ! #Stim` → başlık, tarih, saat, öncelik, proje
  (bugün, yarın, öbür gün, haftaya, pzt…paz, `2 Ara`, `25.11`, `+3`, `3 gün sonra`, `14:00`)
- "Yakında" görünümü: tüm projelerdeki tarihli işler
- Kapat (X) → tray'e gizlenir; tray sağ tık → Ana pencere / Windows ile başlat / Çık
- Tray paneli: sol tık veya **Ctrl+Space** → geciken ve 7 gün içindeki işler, proje seçerek hızlı ekleme
- Tek örnek; Windows açılışında gizli başlar (`--hidden`)
- Veriler: `%APPDATA%\YarinYaparim\veri.db` (SQLite)

## Derleme (Windows)
```
rustup default stable
cargo build --release
```
Çıktı: `target\release\YarinYaparim.exe`

Linux'tan Windows'a çapraz derleme MinGW ile yapılabilir (`x86_64-pc-windows-gnu`).

## Yapı
- `src/main.rs` – uygulama mantığı, pencereler, tray olayları
- `src/db.rs` – SQLite şeması ve sorgular
- `src/parse.rs` – Türkçe tarih/saat ayrıştırıcı (`cargo test`)
- `src/platform.rs` – Windows: tray, kısayol, otomatik başlatma, tek örnek
- `ui/app.slint` – arayüz (ana pencere + tray paneli)

## Yol haritası
- [x] Ana pencere, projeler, görevler
- [x] Tray, otomatik başlatma, tray paneli, kısayol
- [ ] Deadline bildirimleri
- [ ] MCP sunucusu (Claude'un görev ekleyebilmesi için)

## Lisanslar
Fontlar (Overpass, Literata) SIL Open Font License 1.1 ile dağıtılır; bkz. `ui/fonts/*-OFL.txt`.
