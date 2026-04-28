import "./styles.css";

const app = document.querySelector<HTMLDivElement>("#app");

if (app) {
  app.innerHTML = `
    <section class="shell">
      <p class="eyebrow">FreeLip internal MVP</p>
      <h1>Dev scaffold ready</h1>
      <p>
        Tauri, Rust, Python, and shared contract schemas are scaffolded for the
        local Windows VSR research loop. Runtime features land in later tasks.
      </p>
    </section>
  `;
}
