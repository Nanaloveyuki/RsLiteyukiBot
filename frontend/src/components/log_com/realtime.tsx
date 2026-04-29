import { Button } from '@heroui/button';
import type { Selection } from '@react-types/shared';
import { useLocalStorage } from '@uidotdev/usehooks';
import clsx from 'clsx';
import { useEffect, useRef, useState } from 'react';
import toast from 'react-hot-toast';
import { BsArrowDownCircle } from 'react-icons/bs';
import { IoDownloadOutline } from 'react-icons/io5';
import { Terminal } from '@xterm/xterm';

import key from '@/const/key';
import { colorizeLogLevel } from '@/utils/terminal';

import WebUIManager, { Log } from '@/controllers/webui_manager';

import type { XTermRef } from '../xterm';
import XTerm from '../xterm';
import LogLevelSelect from './log_level_select';

const RealTimeLogs = () => {
  const Xterm = useRef<XTermRef>(null);
  const [logLevel, setLogLevel] = useState<Selection>(
    new Set(['info', 'warn', 'error'])
  );
  const [dataArr, setDataArr] = useState<Log[]>([]);
  const [isFollowing, setIsFollowing] = useState(true);
  const [backgroundImage] = useLocalStorage<string>(key.backgroundImage, '');
  const hasBackground = !!backgroundImage;
  const isFollowingRef = useRef(true);

  const renderLogs = async () => {
    const terminal = Xterm.current?.terminalRef.current;
    if (!terminal) {
      return;
    }

    try {
      const previousBuffer = terminal.buffer.active;
      const distanceFromBottom = Math.max(
        previousBuffer.baseY - previousBuffer.viewportY,
        0
      );
      const content = dataArr
        .filter((log) => {
          if (logLevel === 'all') {
            return true;
          }
          return logLevel.has(log.level);
        })
        .map((log) => colorizeLogLevel(log.message).content)
        .join('\r\n');

      Xterm.current?.clear();
      if (content) {
        await Xterm.current?.writeAsync(content);
      }

      if (isFollowingRef.current) {
        terminal.scrollToBottom();
        return;
      }

      const nextBaseY = terminal.buffer.active.baseY;
      terminal.scrollToLine(Math.max(nextBaseY - distanceFromBottom, 0));
    } catch (error) {
      console.error(error);
      toast.error('获取实时日志失败');
    }
  };

  const onDownloadLog = () => {
    const logContent = dataArr
      .filter((log) => {
        if (logLevel === 'all') {
          return true;
        }
        return logLevel.has(log.level);
      })
      .map((log) => colorizeLogLevel(log.message).content)
      .join('\r\n');
    const blob = new Blob([logContent], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'napcat.log';
    a.click();
    URL.revokeObjectURL(url);
  };

  useEffect(() => {
    void renderLogs();
  }, [logLevel, dataArr]);

  useEffect(() => {
    const subscribeLogs = () => {
      try {
        const source = WebUIManager.getRealTimeLogs((data) => {
          setDataArr((prev) => {
            const newData = [...prev, ...data];
            if (newData.length > 1000) {
              newData.splice(0, newData.length - 1000);
            }
            return newData;
          });
        });
        return () => {
          source.close();
        };
      } catch (_error) {
        toast.error('获取实时日志失败');
      }
    };

    const close = subscribeLogs();
    return () => {
      console.log('close');
      close?.();
    };
  }, []);

  const handleViewportChange = ({ atBottom }: { atBottom: boolean; }) => {
    isFollowingRef.current = atBottom;
    setIsFollowing(atBottom);
  };

  const handleTerminalReady = (terminal: Terminal) => {
    terminal.scrollToBottom();
  };

  const resumeFollowing = () => {
    const terminal = Xterm.current?.terminalRef.current;
    if (!terminal) {
      return;
    }
    terminal.scrollToBottom();
    isFollowingRef.current = true;
    setIsFollowing(true);
  };

  return (
    <>
      <title>实时日志 - Liteyuki WebUI</title>
      <div className={clsx(
        'flex items-center gap-2 p-2 rounded-2xl border backdrop-blur-sm transition-all shadow-sm mb-4',
        hasBackground ? 'bg-white/20 dark:bg-black/10 border-white/40 dark:border-white/10' : 'bg-white/60 dark:bg-black/40 border-white/40 dark:border-white/10'
      )}
      >
        <LogLevelSelect
          selectedKeys={logLevel}
          onSelectionChange={setLogLevel}
        />
        <Button
          className='flex-shrink-0'
          onPress={onDownloadLog}
          startContent={<IoDownloadOutline className='text-lg' />}
          color='primary'
          variant='flat'
        >
          下载日志
        </Button>
        {!isFollowing && (
          <Button
            className='flex-shrink-0'
            onPress={resumeFollowing}
            startContent={<BsArrowDownCircle className='text-base' />}
            color='warning'
            variant='flat'
          >
            回到底部
          </Button>
        )}
      </div>
      <div className='flex-1 h-full overflow-hidden'>
        <XTerm
          ref={Xterm}
          onTerminalReady={handleTerminalReady}
          onViewportChange={handleViewportChange}
        />
      </div>
    </>
  );
};

export default RealTimeLogs;
