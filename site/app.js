const tabs = Array.from(document.querySelectorAll("[data-tab]"));
const panels = Array.from(document.querySelectorAll("[data-panel]"));

function activateTab(name) {
  tabs.forEach((tab) => {
    const isActive = tab.dataset.tab === name;
    tab.classList.toggle("is-active", isActive);
    tab.setAttribute("aria-selected", String(isActive));
  });

  panels.forEach((panel) => {
    const isActive = panel.dataset.panel === name;
    panel.classList.toggle("is-active", isActive);
    panel.hidden = !isActive;
  });
}

tabs.forEach((tab) => {
  tab.addEventListener("click", () => {
    activateTab(tab.dataset.tab);
    history.replaceState(null, "", `#tab-${tab.dataset.tab}`);
  });
});

function activateTabFromHash() {
  const tabName = window.location.hash.replace(/^#tab-/, "");
  if (tabName && tabs.some((tab) => tab.dataset.tab === tabName)) {
    activateTab(tabName);
  }
}

activateTabFromHash();
window.addEventListener("hashchange", activateTabFromHash);

const slides = Array.from(document.querySelectorAll("[data-slide]"));
const dots = Array.from(document.querySelectorAll("[data-carousel-dot]"));
let currentSlide = 0;

function activateSlide(index) {
  currentSlide = (index + slides.length) % slides.length;

  slides.forEach((slide, slideIndex) => {
    slide.classList.toggle("is-active", slideIndex === currentSlide);
  });

  dots.forEach((dot, dotIndex) => {
    dot.classList.toggle("is-active", dotIndex === currentSlide);
  });
}

document.querySelectorAll("[data-carousel-action]").forEach((button) => {
  button.addEventListener("click", () => {
    const offset = button.dataset.carouselAction === "next" ? 1 : -1;
    activateSlide(currentSlide + offset);
  });
});

dots.forEach((dot) => {
  dot.addEventListener("click", () => {
    activateSlide(Number(dot.dataset.carouselDot));
  });
});
