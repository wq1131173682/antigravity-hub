import { Sun, Moon, LogOut, Minimize2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { MoreDropdown } from './NavDropdowns';
import { isTauri } from '../../utils/env';
import { useViewStore } from '../../stores/useViewStore';

interface NavSettingsProps {
    theme: 'light' | 'dark';
    onThemeToggle: (event: React.MouseEvent<HTMLButtonElement>) => void;
}

/**
 * 设置按钮组件 - 独立处理响应式
 *
 * 响应式策略:
 * - ≥ 480px: 独立按钮(主题 + 迷你视图)
 * - < 480px: 更多下拉菜单
 */
export function NavSettings({
    theme,
    onThemeToggle,
}: NavSettingsProps) {
    const { t } = useTranslation();
    const { setMiniView } = useViewStore();

    const handleLogout = () => {
        sessionStorage.removeItem('abv_admin_api_key');
        localStorage.removeItem('abv_admin_api_key');
        window.location.reload();
    };

    return (
        <>
            {/* 独立按钮 (≥ 480px) */}
            <div className="hidden min-[480px]:flex items-center gap-2">
                {/* 迷你视图切换按钮 */}
                <button
                    onClick={() => setMiniView(true)}
                    className="w-10 h-10 rounded-full bg-gray-100 dark:bg-base-200 hover:bg-gray-200 dark:hover:bg-base-100 flex items-center justify-center transition-colors"
                    title={t('nav.mini_view', 'Mini View')}
                >
                    <Minimize2 className="w-5 h-5 text-gray-700 dark:text-gray-300" />
                </button>

                {/* 主题切换按钮 */}
                <button
                    onClick={onThemeToggle}
                    className="w-10 h-10 rounded-full bg-gray-100 dark:bg-base-200 hover:bg-gray-200 dark:hover:bg-base-100 flex items-center justify-center transition-colors"
                    title={theme === 'light' ? t('nav.theme_to_dark') : t('nav.theme_to_light')}
                >
                    {theme === 'light' ? (
                        <Moon className="w-5 h-5 text-gray-700 dark:text-gray-300" />
                    ) : (
                        <Sun className="w-5 h-5 text-gray-700 dark:text-gray-300" />
                    )}
                </button>

                {/* 登出按钮 - 仅 Web 模式显示 */}
                {!isTauri() && (
                    <button
                        onClick={handleLogout}
                        className="w-10 h-10 rounded-full bg-red-50 dark:bg-red-900/20 hover:bg-red-100 dark:hover:bg-red-900/40 flex items-center justify-center transition-colors"
                        title={t('nav.logout', '登出')}
                    >
                        <LogOut className="w-5 h-5 text-red-600 dark:text-red-400" />
                    </button>
                )}
            </div>

            {/* 更多菜单 (< 480px) */}
            <div className="min-[480px]:hidden">
                <MoreDropdown
                    theme={theme}
                    onThemeToggle={onThemeToggle}
                />
            </div>
        </>
    );
}
