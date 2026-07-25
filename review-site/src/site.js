const languageKey = "mine-mail-site-language";
const body = document.body;
const languageButton = document.querySelector("[data-language-toggle]");
const menuButton = document.querySelector("[data-menu-toggle]");
const navigation = document.querySelector("[data-navigation]");

function preferredLanguage() {
  const requested = new URLSearchParams(window.location.search).get("lang");
  if (requested === "en" || requested === "zh") return requested;

  const saved = window.localStorage.getItem(languageKey);
  if (saved === "en" || saved === "zh") return saved;

  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

function setLanguage(language) {
  body.dataset.language = language;
  document.documentElement.lang = language === "zh" ? "zh-CN" : "en";
  document.title =
    language === "zh" ? body.dataset.titleZh : body.dataset.titleEn;
  languageButton?.setAttribute(
    "aria-label",
    language === "zh" ? "Switch to English" : "切换到中文",
  );
  window.localStorage.setItem(languageKey, language);
}

setLanguage(preferredLanguage());

languageButton?.addEventListener("click", () => {
  setLanguage(body.dataset.language === "zh" ? "en" : "zh");
});

menuButton?.addEventListener("click", () => {
  const open = navigation?.dataset.open !== "true";
  if (navigation) navigation.dataset.open = String(open);
  menuButton.setAttribute("aria-expanded", String(open));
});

navigation?.addEventListener("click", (event) => {
  if (event.target.closest("a")) {
    navigation.dataset.open = "false";
    menuButton?.setAttribute("aria-expanded", "false");
  }
});

document.querySelectorAll("[data-year]").forEach((node) => {
  node.textContent = String(new Date().getFullYear());
});

