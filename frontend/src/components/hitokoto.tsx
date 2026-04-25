/* eslint-disable @stylistic/jsx-closing-bracket-location */
/* eslint-disable @stylistic/jsx-closing-tag-location */
import { Button } from '@heroui/button';
import { Tooltip } from '@heroui/tooltip';
import { useLocalStorage } from '@uidotdev/usehooks';
import clsx from 'clsx';
import { useState } from 'react';
import toast from 'react-hot-toast';
import { IoMdQuote } from 'react-icons/io';
import { IoCopy, IoRefresh } from 'react-icons/io5';

import key from '@/const/key';

const HITOKOTO_POOL: Array<Pick<IHitokoto, 'hitokoto' | 'from' | 'from_who'>> = [
  {
    hitokoto: '凡是过往，皆为序章。',
    from: '暴风雨',
    from_who: '莎士比亚',
  },
  {
    hitokoto: '自然选择，前进四！',
    from: '自定义',
    from_who: 'Liteyuki',
  },
  {
    hitokoto: '铭刻在心：每一天都是一年中最好的日子。',
    from: '语录',
    from_who: '爱默生',
  },
  {
    hitokoto: '一个人的价值，在于他贡献了什么，而不在于他能得到什么。',
    from: '语录',
    from_who: '爱因斯坦',
  },
  {
    hitokoto: '智慧并不产生于学历，而是来自对于知识的终身不懈的追求。',
    from: '教育论',
    from_who: '爱因斯坦',
  },
  {
    hitokoto: '未经反思自省的人生没有意义。',
    from: '自辩辞',
    from_who: '苏格拉底',
  },
  {
    hitokoto: '少关心别人的逸闻私事，多留意别人的思路观点。',
    from: '语录',
    from_who: '居里夫人',
  },
  {
    hitokoto: '生命是永恒不断的创造。',
    from: '语录',
    from_who: '泰戈尔',
  },
  {
    hitokoto: '生活是黑暗中眨眼间的光。',
    from: '风之谷',
    from_who: '宫崎骏',
  },
  {
    hitokoto: '千里之行，始于足下。',
    from: '道德经',
    from_who: '老子',
  },
  {
    hitokoto: '上善若水，水善利万物而不争。',
    from: '道德经',
    from_who: '老子',
  },
  {
    hitokoto: '民有、民治、民享。',
    from: '葛底斯堡演说',
    from_who: '林肯',
  },
  {
    hitokoto: '文学就是我的天国。',
    from: '我的生活',
    from_who: '海伦·凯勒',
  },
];

function pickRandomHitokoto (current?: string) {
  if (HITOKOTO_POOL.length <= 1) {
    return HITOKOTO_POOL[0];
  }

  let next = HITOKOTO_POOL[Math.floor(Math.random() * HITOKOTO_POOL.length)];

  while (next.hitokoto === current) {
    next = HITOKOTO_POOL[Math.floor(Math.random() * HITOKOTO_POOL.length)];
  }

  return next;
}

export default function Hitokoto () {
  const [data, setData] = useState(() => pickRandomHitokoto());
  const [backgroundImage] = useLocalStorage<string>(key.backgroundImage, '');
  const hasBackground = !!backgroundImage;

  const onRefresh = () => {
    setData(current => pickRandomHitokoto(current.hitokoto));
  };

  const onCopy = () => {
    try {
      const text = `${data?.hitokoto} —— ${data?.from} ${data?.from_who}`;
      navigator.clipboard.writeText(text);
      toast.success('复制成功');
    } catch (_error) {
      toast.error('复制失败, 请手动复制');
    }
  };
  return (
    <div className='overflow-hidden'>
      <div className='relative flex flex-col items-center justify-center p-4 md:p-6'>
        {data && (
          <>
            <IoMdQuote className={clsx(
              'text-4xl mb-4',
              hasBackground ? 'text-white/30' : 'text-primary/20'
            )}
            />
            <div className={clsx(
              'text-xl font-medium tracking-wide leading-relaxed italic',
              hasBackground ? 'text-white drop-shadow-sm' : 'text-default-700 dark:text-gray-200'
            )}
            >
              " {data?.hitokoto} "
            </div>
            <div className='mt-4 flex flex-col items-center text-sm'>
              <span className={clsx(
                'font-bold',
                hasBackground ? 'text-white/90' : 'text-primary-500/80'
              )}
              >—— {data?.from}
              </span>
              {data?.from_who && <span className={clsx(
                'text-xs mt-1',
                hasBackground ? 'text-white/70' : 'text-default-400'
              )}
              >                {data?.from_who}
              </span>}
            </div>
          </>
        )}
      </div>
      <div className='flex gap-2'>
        <Tooltip content='刷新' placement='top'>
          <Button
            className={clsx(
              'transition-colors',
              hasBackground ? 'text-white/60 hover:text-white' : 'text-default-400 hover:text-primary'
            )}
            onPress={onRefresh}
            size='sm'
            isIconOnly
            radius='full'
            variant='light'
          >
            <IoRefresh />
          </Button>
        </Tooltip>
        <Tooltip content='复制' placement='top'>
          <Button
            className={clsx(
              'transition-colors',
              hasBackground ? 'text-white/60 hover:text-white' : 'text-default-400 hover:text-success'
            )}
            onPress={onCopy}
            size='sm'
            isIconOnly
            radius='full'
            variant='light'
          >
            <IoCopy />
          </Button>
        </Tooltip>
      </div>
    </div>
  );
}
