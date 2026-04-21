import type { ReactNode } from "react";

import {
  LuActivity,
  LuBot,
  LuFileText,
  LuInfo,
  LuLayoutDashboard,
  LuPackage,
  LuSignal,
  LuTerminal,
} from "react-icons/lu";

export interface MenuItem {
  id: string;
  label: string;
  href: string;
  icon: ReactNode;
}

export const siteConfig = {
  name: "RsLiteyukiBot",
  description: "Tauri2 desktop shell powered by a shared Rust runtime.",
  links: {
    repo: "https://github.com/LiteyukiStudio/RsLiteyukiBot",
  },
  navItems: [
    {
      id: "overview",
      label: "基础信息",
      href: "/",
      icon: <LuLayoutDashboard className="h-5 w-5" />,
    },
    {
      id: "runtime",
      label: "运行信息",
      href: "/runtime",
      icon: <LuActivity className="h-5 w-5" />,
    },
    {
      id: "adapters",
      label: "网络信息",
      href: "/adapters",
      icon: <LuSignal className="h-5 w-5" />,
    },
    {
      id: "llm",
      label: "对话模式",
      href: "/llm",
      icon: <LuBot className="h-5 w-5" />,
    },
    {
      id: "commands",
      label: "命令中心",
      href: "/commands",
      icon: <LuTerminal className="h-5 w-5" />,
    },
    {
      id: "plugins",
      label: "插件面板",
      href: "/plugins",
      icon: <LuPackage className="h-5 w-5" />,
    },
    {
      id: "logs",
      label: "日志信息",
      href: "/logs",
      icon: <LuFileText className="h-5 w-5" />,
    },
    {
      id: "diagnostics",
      label: "诊断信息",
      href: "/diagnostics",
      icon: <LuInfo className="h-5 w-5" />,
    },
  ] as MenuItem[],
};
