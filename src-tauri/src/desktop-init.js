(() => {
  if (window.location.origin !== __DOMINION_ORIGIN__) return;
  Object.defineProperty(window, '__DOMINION_NATIVE__', {value: true});
  document.addEventListener('contextmenu', event => {
    const invoke = window.__TAURI_INTERNALS__?.invoke;
    if (!invoke) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    invoke('native_context_menu').catch(error => console.error('Native menu:', error));
  }, true);
})();
