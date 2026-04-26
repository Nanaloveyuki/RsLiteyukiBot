import {
  LuActivity,
  LuFileText,
  LuFolderOpen,
  LuInfo,
  LuLayoutDashboard,
  LuSettings,
  LuSignal,
  LuTerminal,
  LuZap,
  LuPackage,
  LuStore,
  LuPuzzle,
  LuBraces,
} from 'react-icons/lu';

export type SiteConfig = typeof siteConfig;
export interface MenuItem {
  label: string;
  icon?: React.ReactNode;
  autoOpen?: boolean;
  href?: string;
  items?: MenuItem[];
  customIcon?: string;
}

export const siteConfig = {
  name: 'LiteyukiBot',
  description: 'LiteyukiBot WebUI.',
  navItems: [
    {
      label: '基础信息',
      icon: <LuLayoutDashboard className='w-5 h-5' />,
      href: '/',
    },
    {
      label: '网络配置',
      icon: <LuSignal className='w-5 h-5' />,
      href: '/network',
    },
    {
      label: '猫猫日志',
      icon: <LuFileText className='w-5 h-5' />,
      href: '/logs',
    },
    {
      label: '模型管理',
      icon: <LuActivity className='w-5 h-5' />,
      href: '/debug/http',
    },
    {
      label: '模型对话',
      icon: <LuZap className='w-5 h-5' />,
      href: '/debug/ws',
    },
    {
      label: '能力面板',
      icon: <LuBraces className='w-5 h-5' />,
      href: '/capabilities',
    },
    {
      label: '文件管理',
      icon: <LuFolderOpen className='w-5 h-5' />,
      href: '/file_manager',
    },
    {
      label: '插件管理',
      icon: <LuPackage className='w-5 h-5' />,
      href: '/plugins',
    },
    {
      label: '插件商店',
      icon: <LuStore className='w-5 h-5' />,
      href: '/plugin_store',
    },
    {
      label: '扩展页面',
      icon: <LuPuzzle className='w-5 h-5' />,
      href: '/extension',
    },
    {
      label: '系统终端',
      icon: <LuTerminal className='w-5 h-5' />,
      href: '/terminal',
    },
    {
      label: '系统配置',
      icon: <LuSettings className='w-5 h-5' />,
      href: '/config',
    },
    {
      label: '关于我们',
      icon: <LuInfo className='w-5 h-5' />,
      href: '/about',
    },
  ] as MenuItem[],
  links: {
    github: 'https://github.com/LiteyukiStudio/RsLiteyukiBot',
    docs: 'https://bot.liteyuki.icu/',
  },
};
