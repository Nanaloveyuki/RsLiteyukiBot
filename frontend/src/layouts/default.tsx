import { BreadcrumbItem, Breadcrumbs } from "@heroui/breadcrumbs";
import { Button } from "@heroui/button";
import clsx from "clsx";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useEffectEvent, useMemo, useRef, useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { MdMenu, MdMenuOpen } from "react-icons/md";

import PageBackground from "@/components/page_background";
import SideBar from "@/components/sidebar";
import { siteConfig } from "@/config/site";
import { useRuntimeHealthContext } from "@/contexts/runtime-health";
import { useThemeMode } from "@/contexts/theme-mode";

export default function Layout() {
  const [openSideBar, setOpenSideBar] = useState(true);
  const [topbarVisible, setTopbarVisible] = useState(true);
  const contentRef = useRef<HTMLDivElement>(null);
  const lastScrollTopRef = useRef(0);
  const location = useLocation();
  const navigate = useNavigate();
  const { refresh } = useRuntimeHealthContext();
  const { isDark, toggleTheme } = useThemeMode();

  const handleContentScroll = useEffectEvent(() => {
    const container = contentRef.current;

    if (!container) {
      return;
    }

    const nextScrollTop = container.scrollTop;
    const scrollDelta = nextScrollTop - lastScrollTopRef.current;

    if (nextScrollTop <= 20) {
      setTopbarVisible(true);
      lastScrollTopRef.current = nextScrollTop;
      return;
    }

    if (scrollDelta > 10) {
      setTopbarVisible(false);
    } else if (scrollDelta < -10) {
      setTopbarVisible(true);
    }

    lastScrollTopRef.current = nextScrollTop;
  });

  useEffect(() => {
    contentRef.current?.scrollTo?.({
      top: 0,
      behavior: "smooth",
    });
    lastScrollTopRef.current = 0;
    setTopbarVisible(true);
  }, [location.pathname]);

  useEffect(() => {
    const container = contentRef.current;

    if (!container) {
      return;
    }

    const onScroll = () => handleContentScroll();

    container.addEventListener("scroll", onScroll, { passive: true });

    return () => {
      container.removeEventListener("scroll", onScroll);
    };
  }, []);

  const activeItem = useMemo(
    () => siteConfig.navItems.find((item) => item.href === location.pathname) ?? siteConfig.navItems[0],
    [location.pathname],
  );

  const titleTrail = useMemo(() => ["控制台", activeItem.label], [activeItem.label]);

  return (
    <div className="relative flex h-screen items-stretch overflow-hidden">
      <PageBackground />
      <SideBar
        activePath={location.pathname}
        items={siteConfig.navItems}
        isDark={isDark}
        onClose={() => setOpenSideBar(false)}
        onNavigate={(href) => navigate(href)}
        onRefresh={() => void refresh()}
        onToggleTheme={toggleTheme}
        open={openSideBar}
      />
      <motion.div
        layout
        ref={contentRef}
        initial={{ opacity: 0, scale: 0.98 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.4 }}
        className={clsx("flex-1 overflow-y-auto pb-10 transition-all duration-300 ease-in-out md:pb-0")}
      >
        <motion.div
          className={clsx("sticky left-0 top-0 z-30 px-2 pt-2 md:px-4", !topbarVisible && "pointer-events-none")}
          animate={{
            opacity: topbarVisible ? 1 : 0,
            y: topbarVisible ? 0 : -96,
          }}
          transition={{ duration: 0.26, ease: [0.22, 1, 0.36, 1] }}
        >
          <div className="panel-surface-strong flex h-11 items-center rounded-2xl px-2.5 shadow-sm shadow-cyan-100/50 dark:shadow-black/20">
            <div className={clsx("z-50 mr-1 ml-0 ease-in-out md:z-auto md:!ml-0 md:pl-0", openSideBar && "pl-2")}>
              <Button isIconOnly radius="full" variant="light" onPress={() => setOpenSideBar(!openSideBar)}>
                {openSideBar ? <MdMenuOpen size={24} /> : <MdMenu size={24} />}
              </Button>
            </div>
            <div className="min-w-0 flex-1 overflow-hidden">
              <Breadcrumbs isDisabled size="lg">
                {titleTrail.map((item) => (
                  <BreadcrumbItem key={item}>
                    <AnimatePresence mode="wait">
                      <motion.div
                        key={item}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: 10 }}
                        initial={{ opacity: 0, y: -10 }}
                        transition={{ duration: 0.3 }}
                      >
                        {item}
                      </motion.div>
                    </AnimatePresence>
                  </BreadcrumbItem>
                ))}
              </Breadcrumbs>
            </div>
          </div>
        </motion.div>
        <div className="px-2 pt-4 md:px-4 md:pt-5">
          <Outlet />
        </div>
      </motion.div>
    </div>
  );
}
