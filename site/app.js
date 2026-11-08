const THEME_PREFERENCE_KEY = "sol-validator-watcher-theme";
const themeMediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
const themeColor = document.querySelector('meta[name="theme-color"]');
const themeToggles = Array.from(document.querySelectorAll("[data-theme-toggle]"));

function readStoredTheme() {
  try {
    const storedTheme = window.localStorage.getItem(THEME_PREFERENCE_KEY);
    return storedTheme === "light" || storedTheme === "dark" ? storedTheme : null;
  } catch (error) {
    console.warn("Unable to read the saved theme preference; using the system theme.", error);
    return null;
  }
}

function storeTheme(theme) {
  try {
    window.localStorage.setItem(THEME_PREFERENCE_KEY, theme);
  } catch (error) {
    console.warn("Unable to save the theme preference; keeping it for this page.", error);
  }
}

function applyTheme(theme) {
  const selectedTheme = theme === "dark" ? "dark" : "light";
  const isDark = selectedTheme === "dark";
  const nextTheme = isDark ? "light" : "dark";
  const actionLabel = `Switch to ${nextTheme} theme`;

  document.documentElement.dataset.theme = selectedTheme;
  document.documentElement.style.colorScheme = selectedTheme;
  themeColor?.setAttribute("content", isDark ? "#0d1117" : "#f4f1e8");

  themeToggles.forEach((toggle) => {
    toggle.setAttribute("aria-label", actionLabel);
    toggle.setAttribute("aria-pressed", String(isDark));
    toggle.setAttribute("title", actionLabel);
  });
}

const storedTheme = readStoredTheme();
let hasExplicitTheme = storedTheme !== null;
applyTheme(
  storedTheme ??
    document.documentElement.dataset.theme ??
    (themeMediaQuery.matches ? "dark" : "light"),
);

themeToggles.forEach((toggle) => {
  toggle.addEventListener("click", () => {
    const nextTheme = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    hasExplicitTheme = true;
    applyTheme(nextTheme);
    storeTheme(nextTheme);
  });
});

themeMediaQuery.addEventListener("change", (event) => {
  if (!hasExplicitTheme) {
    applyTheme(event.matches ? "dark" : "light");
  }
});

const dashboardTabs = Array.from(document.querySelectorAll("[data-dashboard-tab]"));
const dashboardPanels = Array.from(document.querySelectorAll("[data-dashboard-panel]"));

function activateDashboard(name, moveFocus = false) {
  dashboardTabs.forEach((tab) => {
    const isActive = tab.dataset.dashboardTab === name;
    tab.classList.toggle("is-active", isActive);
    tab.setAttribute("aria-selected", String(isActive));
    tab.tabIndex = isActive ? 0 : -1;

    if (isActive && moveFocus) {
      tab.focus();
    }
  });

  dashboardPanels.forEach((panel) => {
    const isActive = panel.dataset.dashboardPanel === name;
    panel.classList.toggle("is-active", isActive);
    panel.hidden = !isActive;
  });
}

dashboardTabs.forEach((tab, index) => {
  tab.addEventListener("click", () => activateDashboard(tab.dataset.dashboardTab));

  tab.addEventListener("keydown", (event) => {
    let nextIndex;

    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        nextIndex = (index + 1) % dashboardTabs.length;
        break;
      case "ArrowLeft":
      case "ArrowUp":
        nextIndex = (index - 1 + dashboardTabs.length) % dashboardTabs.length;
        break;
      case "Home":
        nextIndex = 0;
        break;
      case "End":
        nextIndex = dashboardTabs.length - 1;
        break;
      default:
        return;
    }

    event.preventDefault();
    activateDashboard(dashboardTabs[nextIndex].dataset.dashboardTab, true);
  });
});
