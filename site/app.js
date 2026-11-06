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
