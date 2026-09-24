# parked/ — derlenmeyen, silinmemiş kod

Bu dizindeki kod **workspace'in dışında**: derlenmez, test edilmez, CI'a
girmez (`Cargo.toml` → `exclude`). `spike/`'tan farkı: buradaki kod atılabilir
bir prototip değil, çalışmış ve yerine dönmesi beklenen bir parça.

## `crates/headshell-plugin-torrent/` + `plugins/torrent/`

Torrent sağlayıcısı (D-047): Torznab'da arar, `librqbit` ile sıralı indirip
yerel bir adresten çalar. **D-069 ile park edildi.**

Sebebi: eklenti sistemi alt süreç + JSON-RPC'den (api 1) gömülü QuickJS'e
(api 2) taşındı. Torrent eklentisi api 1'e yazılmış bir Rust ikilisiydi ve
çekirdeğin `plugin::protocol` tiplerini kullanıyordu; o tipler kalkınca
derlenmez hâle geldi. Kullanıcının kararı: *"torrent'i şimdilik boşverelim,
bizimle gelmesin; ona çok daha sonra bakarız."*

Kod olduğu gibi duruyor — 2.335 satır kaynak, 647 satır test (D-056'nın
ölçümü). Geri dönüş için açık sorular:

1. **Nerede koşacak?** `librqbit` bir BitTorrent istemcisi: rastgele
   peer'lara soket açıyor. QuickJS motorunun `host.http`'si bunu ifade
   edemez. Üç yol görünüyor: motorun kurduğu bir **araç** olarak
   (`host.tools.run`, platform başına önceden derlenmiş ikili — yt-dlp'nin
   yolu), çekirdekte feature kapılı bir sağlayıcı olarak (D-050 S3'ün
   iptal edilen fikri, +179 crate), ya da ayrı bir sağlayıcı sınıfı.
2. **Dağıtım.** Hangi yol seçilirse seçilsin, kullanıcıya `cargo build`
   yaptırmak D-049'u ihlal eder — bu borç park edilmeden önce de vardı.
3. **İzinler.** api 2'de ağ izni zorlanıyor; bir torrent istemcisinin
   bağlanacağı adresler önceden bilinemez. Motorun izin sözlüğü bunu bugün
   ifade edemiyor ve etmemeli de — bu bir araç ya da çekirdek sağlayıcısı
   olmanın gerekçesi.

Karar verilmeden kod workspace'e geri alınmaz.
