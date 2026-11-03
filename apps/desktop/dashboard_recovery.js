(() => {
  if (window !== window.top || window.__GAH_RECOVERY_WATCHDOG__) return;
  window.__GAH_RECOVERY_WATCHDOG__ = true;
  window.setTimeout(() => {
    const root = document.getElementById('root');
    // Leave a rendered app, its own error state, and non-dashboard documents alone.
    if (!root || root.children.length || root.textContent.trim()) return;
    const recovery = document.createElement('section');
    recovery.className = 'gah-dashboard-recovery';
    recovery.setAttribute('aria-labelledby', 'gah-recovery-title');
    recovery.innerHTML = `
      <style>
        .gah-dashboard-recovery { position: fixed; inset: 0; box-sizing: border-box; overflow: auto; display: grid; align-content: center; padding: 2rem; background: #0b0e14; color: #e6e9ef; font: 16px/1.6 system-ui, sans-serif; color-scheme: dark; }
        .gah-dashboard-recovery > div { width: 100%; max-width: 36rem; margin: auto; }
        .gah-dashboard-recovery h1 { font-size: 1.75rem; line-height: 1.25; margin: 0 0 1rem; }
        .gah-dashboard-recovery p { color: #b4bdcc; margin: 0 0 1.5rem; }
        .gah-dashboard-recovery output { display: block; color: #b4bdcc; overflow-wrap: anywhere; margin-bottom: 2rem; }
        .gah-dashboard-recovery nav { display: flex; flex-wrap: wrap; gap: .75rem; }
        .gah-dashboard-recovery button, .gah-dashboard-recovery a { box-sizing: border-box; display: inline-flex; align-items: center; min-height: 44px; padding: .55rem .85rem; border: 1px solid #536078; border-radius: .35rem; font: inherit; cursor: pointer; text-decoration: none; color: #e6e9ef; background: #11151f; }
        .gah-dashboard-recovery button { background: #283c5b; }
        .gah-dashboard-recovery button:hover, .gah-dashboard-recovery a:hover { background: #354e73; }
        .gah-dashboard-recovery :focus-visible { outline: 2px solid #8ab4ff; outline-offset: 3px; }
        .gah-dashboard-recovery h1:focus { outline: none; }
        .gah-dashboard-recovery ::selection { background: #354e73; color: #fff; }
      </style>
      <div>
        <h1 id="gah-recovery-title" tabindex="-1">The dashboard hasn’t started</h1>
        <p>The app files may have failed to load. Try again, or open This computer to check the saved connection.</p>
        <output aria-label="Central address"></output>
        <nav aria-label="Connection recovery">
          <button type="button">Retry dashboard</button>
          <a href="gah://connection-recovery">Open This computer</a>
        </nav>
      </div>`;
    // Show only the public origin, never query strings, fragments or credentials.
    recovery.querySelector('output').textContent = window.location.origin;
    recovery.querySelector('button').addEventListener('click', () => window.location.reload());
    root.append(recovery);
    recovery.querySelector('h1').focus();
  }, 10000);
})();
