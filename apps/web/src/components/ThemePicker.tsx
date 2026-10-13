import { THEMES } from '../lib/themes.js';
import { useUiStore } from '../store/uiStore.js';

/** One button per registered theme; the selected one is pressed. Wraps on narrow screens. */
export function ThemePicker() {
  const { theme, setTheme } = useUiStore();
  return (
    <div role="group" aria-label="Theme" className="flex flex-wrap gap-2 text-sm">
      {THEMES.map(({ id, label, icon: Icon }) => (
        <button
          key={id}
          type="button"
          aria-pressed={theme === id}
          onClick={() => setTheme(id)}
          className={`inline-flex items-center gap-1.5 rounded-md border px-3 py-1.5 ${theme === id ? 'border-transparent bg-accent-fill text-on-fill' : 'border-subtle text-secondary hover:bg-overlay/5'}`}
        >
          <Icon size={14} aria-hidden="true" />
          {label}
        </button>
      ))}
    </div>
  );
}
