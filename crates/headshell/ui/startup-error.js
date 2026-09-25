// The startup error page (D-070). The shell writes the text into the `#` part
// of the address, percent-encoded (`main.rs::encode_fragment`); it is turned
// back here. `textContent`, not `innerHTML`: whatever the error text
// contains, it is not interpreted as markup.
const text = decodeURIComponent(location.hash.slice(1));
document.getElementById("startupError").textContent =
  text || "(no error text arrived — `headshell diag` prints the last run's report)";
