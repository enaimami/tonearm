// Açılış hatası sayfası (D-070). Kabuk metni adresin `#` kısmına yüzde
// kodlamasıyla yazıyor (`main.rs::encode_fragment`); burada geri çevriliyor.
// `textContent`, `innerHTML` değil: hata metni ne içerirse içersin işaretleme
// olarak yorumlanmaz.
const text = decodeURIComponent(location.hash.slice(1));
document.getElementById("startupError").textContent =
  text || "(hata metni gelmedi — `headshell diag` son çalıştırmanın raporunu yazar)";
