import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { Minus, Square, X } from 'lucide-react';

export default function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    win.isMaximized().then(setMaximized);

    let unlistenFn: (() => void) | null = null;

    const setup = async () => {
      unlistenFn = await win.onResized(() => {
        win.isMaximized().then(setMaximized);
      });
    };
    setup();

    return () => {
      if (unlistenFn) unlistenFn();
    };
  }, []);

  const handleMinimize = () => getCurrentWindow().minimize();
  const handleMaximize = async () => {
    const win = getCurrentWindow();
    const isMax = await win.isMaximized();
    if (isMax) {
      await win.unmaximize();
    } else {
      await win.maximize();
    }
    setMaximized(!isMax);
  };
  const handleClose = () => invoke('close_window');

  return (
    <div
      className="flex items-center justify-between h-9 flex-shrink-0 select-none border-b border-gray-200/60 dark:border-base-200/60"
      data-tauri-drag-region
      style={{
        zIndex: 9999,
        cursor: 'default',
        userSelect: 'none',
        WebkitUserSelect: 'none',
      }}
    >
      <span className="text-xs text-gray-400 dark:text-gray-500 ml-3 font-medium tracking-wide">
        Antigravity Hub
      </span>
      <div className="flex h-full" onMouseDown={e => e.stopPropagation()}>
        <button
          onClick={handleMinimize}
          className="w-11 h-full flex items-center justify-center text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 hover:bg-gray-200/70 dark:hover:bg-white/10 transition-colors"
          title="Minimize"
        >
          <Minus size={14} />
        </button>
        <button
          onClick={handleMaximize}
          className="w-11 h-full flex items-center justify-center text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 hover:bg-gray-200/70 dark:hover:bg-white/10 transition-colors"
          title={maximized ? 'Restore' : 'Maximize'}
        >
          {maximized ? (
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <rect x="1.5" y="3.5" width="9" height="9" rx="0.5" />
              <rect x="3.5" y="1.5" width="9" height="9" rx="0.5" fill="currentColor" opacity="0.08" />
            </svg>
          ) : (
            <Square size={13} />
          )}
        </button>
        <button
          onClick={handleClose}
          className="w-11 h-full flex items-center justify-center text-gray-500 dark:text-gray-400 hover:text-white dark:hover:text-white hover:bg-red-500 dark:hover:bg-red-500 transition-colors"
          title="Close"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
}
