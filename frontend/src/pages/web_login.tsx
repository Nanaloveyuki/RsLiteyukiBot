import { Button } from '@heroui/button';
import { CardBody, CardHeader } from '@heroui/card';
import { Image } from '@heroui/image';
import { Input } from '@heroui/input';
import { useLocalStorage } from '@uidotdev/usehooks';
import { useEffect, useState } from 'react';
import { toast } from 'react-hot-toast';
import { IoKeyOutline } from 'react-icons/io5';

import logo from '@/assets/images/logo.png';

import key from '@/const/key';

import HoverEffectCard from '@/components/effect_card';
import { title } from '@/components/primitives';
import { ThemeSwitch } from '@/components/theme-switch';

import WebUIManager from '@/controllers/webui_manager';
import PureLayout from '@/layouts/pure';
import { motion } from 'motion/react';

/** Global injected by the Tauri init script — present only in desktop mode. */
declare const __LITEYUKI_LOCAL_TOKEN__: string | undefined;

export default function WebLoginPage () {
  const urlSearchParams = new URLSearchParams(window.location.search);
  const urlToken = urlSearchParams.get('token');
  const [tokenValue, setTokenValue] = useState<string>(urlToken || '');
  const [isLoading, setIsLoading] = useState<boolean>(false);
  /** True while we are attempting the silent auto-login (Tauri token or local-token API). */
  const [isAutoLogging, setIsAutoLogging] = useState<boolean>(true);
  const [, setLocalToken] = useLocalStorage<string>(key.token, '');

  // ── Core login helper ────────────────────────────────────────────────────
  const loginWithRawToken = async (rawToken: string): Promise<boolean> => {
    try {
      const credential = await WebUIManager.loginWithToken(rawToken);
      if (credential) {
        storeCredentialAndNavigate(credential);
        return true;
      }
    } catch {
      // fall through
    }
    return false;
  };

  const onSubmit = async () => {
    if (!tokenValue) {
      toast.error('请输入token');
      return;
    }
    setIsLoading(true);
    try {
      const ok = await loginWithRawToken(tokenValue);
      if (!ok) {
        toast.error('登录失败，请检查token');
      }
    } catch (error) {
      toast.error((error as Error).message);
    } finally {
      setIsLoading(false);
    }
  };

  // ── Keyboard shortcut ────────────────────────────────────────────────────
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Enter' && !isLoading && !isAutoLogging) {
      onSubmit();
    }
  };

  useEffect(() => {
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [tokenValue, isLoading, isAutoLogging]);

  // ── Direct credential store (skips SHA256 + login round-trip) ───────────
  const storeCredentialAndNavigate = (credential: string) => {
    setLocalToken(credential);
    // Use a hard navigation so the fresh localStorage value is read by all
    // hooks (AuthChecker's useLocalStorage won't see the update via React
    // state before the router fires, so a full reload is the safest approach).
    window.location.replace('/webui/');
  };

  // ── Auto-login on mount ──────────────────────────────────────────────────
  useEffect(() => {
    const tryAutoLogin = async () => {
      // 1. URL token (highest priority — e.g. deep-link from CLI)
      //    URL tokens are raw user tokens that must go through the login API.
      if (urlToken) {
        await loginWithRawToken(urlToken);
        return;
      }

      // 2. Tauri injected token (desktop mode — injected by Rust init script).
      //    The injected value is the credential itself — use it directly.
      const tauriToken =
        typeof __LITEYUKI_LOCAL_TOKEN__ !== 'undefined'
          ? __LITEYUKI_LOCAL_TOKEN__
          : (window as any).__LITEYUKI_LOCAL_TOKEN__;
      if (tauriToken) {
        storeCredentialAndNavigate(tauriToken);
        return;
      }

      // 3. Backend local-token endpoint (web browser on localhost).
      //    The backend only returns the token when the TCP peer is loopback.
      //    The returned token is the credential — use it directly.
      try {
        const res = await fetch('/api/auth/local-token', { method: 'GET' });
        if (res.ok) {
          const json = await res.json();
          if (json?.code === 0 && json?.data?.token) {
            storeCredentialAndNavigate(json.data.token);
            return;
          }
        }
      } catch {
        // Not on localhost or backend unavailable — show login form normally.
      }
    };

    tryAutoLogin().finally(() => setIsAutoLogging(false));
  }, []);

  return (
    <>
      <title>WebUI登录 - LiteyukiBot WebUI</title>
      <PureLayout>
        <motion.div
          initial={{ opacity: 0, y: 20, scale: 0.95 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          transition={{ duration: 0.5, type: 'spring', stiffness: 120, damping: 20 }}
          className='w-[608px] max-w-full py-8 px-2 md:px-8 overflow-hidden'
        >
          <HoverEffectCard
            className='items-center gap-4 pt-0 pb-6 bg-default-50'
            maxXRotation={3}
            maxYRotation={3}
          >
            <CardHeader className='inline-block max-w-lg text-center justify-center'>
              <div className='flex items-center justify-center w-full gap-2 pt-10'>
                <Image alt='logo' height='7em' src={logo} />
                <div>
                  <span className={title()}>Web&nbsp;</span>
                  <span className={title({ color: 'violet' })}>
                    Login&nbsp;
                  </span>
                </div>
              </div>
              <ThemeSwitch className='absolute right-4 top-4' />
            </CardHeader>

            <CardBody className='flex gap-5 py-5 px-5 md:px-10'>
              {isAutoLogging && (
                <div className='text-center text-small text-default-600 dark:text-default-400 px-2'>
                  🔐 正在自动登录...
                </div>
              )}
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  onSubmit();
                }}
              >
                {/* Hidden username field helps browsers identify this as a login form */}
                <input
                  type='text'
                  name='username'
                  value='liteyukibot-webui'
                  autoComplete='username'
                  className='absolute -left-[9999px] opacity-0 pointer-events-none'
                  readOnly
                  tabIndex={-1}
                  aria-label='Username'
                />
                <Input
                  isClearable
                  type='password'
                  name='password'
                  autoComplete='current-password'
                  classNames={{
                    label: 'text-black/50 dark:text-white/90',
                    input: [
                      'bg-transparent',
                      'text-black/90 dark:text-white/90',
                      'placeholder:text-default-700/50 dark:placeholder:text-white/60',
                    ],
                    innerWrapper: 'bg-transparent',
                    inputWrapper: [
                      'shadow-xl',
                      'bg-default-100/70',
                      'dark:bg-default/60',
                      'backdrop-blur-xl',
                      'backdrop-saturate-200',
                      'hover:bg-default-0/70',
                      'dark:hover:bg-default/70',
                      'group-data-[focus=true]:bg-default-100/50',
                      'dark:group-data-[focus=true]:bg-default/60',
                      '!cursor-text',
                    ],
                  }}
                  isDisabled={isLoading || isAutoLogging}
                  label='Token'
                  placeholder='请输入token'
                  radius='lg'
                  size='lg'
                  startContent={
                    <IoKeyOutline className='text-black/50 mb-0.5 dark:text-white/90 text-slate-400 pointer-events-none flex-shrink-0' />
                  }
                  value={tokenValue}
                  onChange={(e) => setTokenValue(e.target.value)}
                  onClear={() => setTokenValue('')}
                />
              </form>
              <div className='text-center text-small text-default-600 dark:text-default-400 px-2'>
                💡 提示：请从 LiteyukiBot 启动日志中查看登录密钥
              </div>
              <Button
                className='mx-10 mt-10 text-lg py-7'
                color='primary'
                isLoading={isLoading}
                radius='full'
                size='lg'
                variant='shadow'
                onPress={onSubmit}
              >
                {!isLoading && (
                  <Image
                    alt='logo'
                    classNames={{
                      wrapper: '-ml-8',
                    }}
                    height='2em'
                    src={logo}
                  />
                )}
                登录
              </Button>
            </CardBody>
          </HoverEffectCard>
        </motion.div>
        <div className='mt-6 text-center text-sm' style={{ color: '#FF7FAC' }}>
          UI Designed By NapCatUI
        </div>
      </PureLayout>
    </>
  );
}
