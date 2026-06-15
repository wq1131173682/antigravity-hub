import { Outlet } from 'react-router-dom';
import Navbar from '../navbar/Navbar';
import ToastContainer from '../common/ToastContainer';
import { useViewStore } from '../../stores/useViewStore';
import MiniView from './MiniView';
import { useEffect } from 'react';
import { isTauri } from '../../utils/env';
import { ensureFullViewState } from '../../utils/windowManager';
import TitleBar from './TitleBar';

function Layout() {
    const { isMiniView } = useViewStore();

    // Ensure correct window state when in Full View (not Mini View)
    // This handles the case where the app was closed in Mini View (small size, no decorations)
    // and restarted (defaults to Full View state but keeps last window properties)
    useEffect(() => {
        if (!isMiniView && isTauri()) {
            ensureFullViewState();
        }
    }, [isMiniView]);

    if (isMiniView) {
        return (
            <>
                <ToastContainer />
                <MiniView />
            </>
        );
    }

    return (
        <div className="h-screen flex flex-col bg-[#FAFBFC] dark:bg-base-300">
            <ToastContainer />
            {isTauri() && <TitleBar />}
            <Navbar />
            <main className="flex-1 overflow-hidden flex flex-col relative">
                <Outlet />
            </main>
        </div>
    );
}

export default Layout;
