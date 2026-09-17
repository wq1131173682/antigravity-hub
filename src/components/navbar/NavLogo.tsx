import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import LogoIcon from '../../../src-tauri/icons/icon.png';

export function NavLogo() {
    const { t } = useTranslation();

    return (
        <Link to="/" draggable="false" className="flex w-full min-w-0 items-center gap-2 text-xl font-semibold text-gray-900 dark:text-base-content">
            <img
                src={LogoIcon}
                alt="Logo"
                className="size-8 cursor-pointer active:scale-95 transition-transform"
                draggable="false"
            />

            {/* 父容器宽度 < 200px 隐藏 */}
            <span className="hidden @[200px]/logo:inline text-nowrap">{t('common.app_name', 'Antigravity Hub')}</span>
        </Link>
    );
}
