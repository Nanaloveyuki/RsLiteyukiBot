import { Button } from "@heroui/button";
import clsx from "clsx";
import { AnimatePresence, motion } from "motion/react";
import { LuMoonStar, LuRefreshCcw, LuSunMedium } from "react-icons/lu";

import type { MenuItem } from "@/config/site";

import Menus from "./menus";

interface SideBarProps {
  open: boolean;
  items: MenuItem[];
  activePath: string;
  onNavigate: (href: string) => void;
  onRefresh: () => void;
  onToggleTheme: () => void;
  isDark: boolean;
  onClose?: () => void;
}

export default function SideBar(props: SideBarProps) {
  const { open, items, activePath, onNavigate, onRefresh, onToggleTheme, isDark, onClose } = props;

  return (
    <>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            className="fixed inset-y-0 left-64 right-0 z-40 bg-black/20 backdrop-blur-[1px] md:hidden"
            aria-hidden="true"
            onClick={onClose}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0, transition: { duration: 0.15 } }}
            transition={{ duration: 0.2, delay: 0.15 }}
          />
        )}
      </AnimatePresence>
      <motion.div
        className={clsx(
          "fixed left-0 top-0 z-50 h-full overflow-hidden rounded-r-3xl border-r border-white/45 bg-white/72 shadow-lg shadow-cyan-100/45 backdrop-blur-md dark:border-white/10 dark:bg-slate-950/66 dark:shadow-slate-950/35 md:static md:rounded-none md:border-r-0 md:bg-transparent md:shadow-none md:backdrop-blur-none",
        )}
        initial={{ width: 0 }}
        animate={{ width: open ? "16rem" : 0 }}
        transition={{
          type: open ? "spring" : "tween",
          stiffness: 150,
          damping: open ? 15 : 10,
        }}
        style={{ overflow: "hidden" }}
      >
        <motion.div className="relative float-right z-30 flex h-full w-64 flex-col items-stretch p-4 transition-transform duration-300 ease-in-out">
          <div className="my-8 ml-2 flex items-center justify-start gap-3 px-2">
            <div className="h-5 w-1 rounded-full bg-primary shadow-xs shadow-primary-400/45" />
            <div className="select-none text-xl font-bold tracking-wide text-slate-800 dark:text-white">
              RsLiteyukiBot
            </div>
          </div>
          <div className="hide-scrollbar flex flex-1 flex-col overflow-y-auto px-2">
            <Menus items={items} activePath={activePath} onNavigate={onNavigate} />
            <div className="mb-10 mt-auto flex flex-col gap-3 px-2 md:mb-0">
              <Button
                className="w-full bg-cyan-50/80 font-medium text-slate-700 shadow-xs backdrop-blur-xs transition-all duration-300 hover:bg-cyan-100/90 hover:text-slate-900 hover:shadow-md dark:bg-cyan-400/12 dark:text-white/92 dark:hover:bg-cyan-400/18"
                radius="full"
                variant="flat"
                onPress={onToggleTheme}
                startContent={isDark ? <LuSunMedium size={18} /> : <LuMoonStar size={18} />}
              >
                切换主题
              </Button>
              <Button
                className="w-full bg-slate-100/78 font-medium text-slate-700 shadow-xs backdrop-blur-xs transition-all duration-300 hover:bg-slate-200/88 hover:text-slate-900 hover:shadow-md dark:bg-white/10 dark:text-white/88 dark:hover:bg-white/14"
                radius="full"
                variant="flat"
                onPress={onRefresh}
                startContent={<LuRefreshCcw size={18} />}
              >
                刷新状态
              </Button>
            </div>
          </div>
        </motion.div>
      </motion.div>
    </>
  );
}
