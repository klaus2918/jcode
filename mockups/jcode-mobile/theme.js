const params = new URLSearchParams(window.location.search);
const theme = params.get('theme') || 'neon';

document.body.classList.remove('theme-neon', 'theme-catppuccin');
document.body.classList.add(`theme-${theme}`);
document.body.dataset.theme = theme;

const syncTheme = (url) => {
  const next = new URL(url, window.location.href);
  next.searchParams.set('theme', theme);
  return next.toString();
};

document.querySelectorAll('[data-theme-link]').forEach((link) => {
  const targetTheme = link.getAttribute('data-theme-link');
  const next = new URL(window.location.href);
  next.searchParams.set('theme', targetTheme);
  link.href = next.toString();
  link.classList.toggle('active', targetTheme === theme);
});

document.querySelectorAll('iframe.screen-frame').forEach((iframe) => {
  iframe.src = syncTheme(iframe.getAttribute('src'));
});
