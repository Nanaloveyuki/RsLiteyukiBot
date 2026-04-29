import { Button } from '@heroui/button';
import { Chip } from '@heroui/chip';
import { useLocalStorage } from '@uidotdev/usehooks';
import { useEffect, useState } from 'react';
import { Controller, useForm } from 'react-hook-form';
import toast from 'react-hot-toast';
import { useNavigate } from 'react-router-dom';

import key from '@/const/key';

import SaveButtons from '@/components/button/save_buttons';
import ImageInput from '@/components/input/image_input';
import PageLoading from '@/components/page_loading';

import { siteConfig } from '@/config/site';
import WebUIManager from '@/controllers/webui_manager';
import { applyWebUiAppearanceToStorage } from '@/utils/webui_appearance';

interface WebUiAppearanceFormData {
  backgroundImage: string;
  customIcons: Record<string, string>;
}

const WebUIConfigCard = () => {
  const navigate = useNavigate();
  const [loading, setLoading] = useState(true);
  const [authState, setAuthState] = useState<WebUiAuthState | null>(null);
  const [, setBackgroundImage] = useLocalStorage(key.backgroundImage, '');
  const [, setCustomIconsStorage] = useLocalStorage<Record<string, string>>(
    key.customIcons,
    {}
  );
  const {
    control,
    handleSubmit,
    formState: { isSubmitting },
    setValue,
  } = useForm<WebUiAppearanceFormData>({
    defaultValues: {
      backgroundImage: '',
      customIcons: {},
    },
  });

  const applyAppearance = (appearance: WebUIAppearanceState) => {
    setValue('backgroundImage', appearance.backgroundImage ?? '');
    setValue('customIcons', appearance.customIcons ?? {});
    applyWebUiAppearanceToStorage(appearance);
    setBackgroundImage(appearance.backgroundImage ?? '');
    setCustomIconsStorage(appearance.customIcons ?? {});
  };

  const loadState = async (showTip = false) => {
    try {
      setLoading(true);
      const [appearance, nextAuthState] = await Promise.all([
        WebUIManager.getWebUIAppearance(),
        WebUIManager.getAuthState(),
      ]);
      applyAppearance(appearance);
      setAuthState(nextAuthState);
      if (showTip) {
        toast.success('刷新成功');
      }
    } catch (error) {
      toast.error(`加载失败: ${(error as Error).message}`);
    } finally {
      setLoading(false);
    }
  };

  const onSubmit = handleSubmit(async (data) => {
    try {
      const appearance = await WebUIManager.updateWebUIAppearance({
        backgroundImage: data.backgroundImage ?? '',
        customIcons: data.customIcons ?? {},
      });
      applyAppearance(appearance);
      toast.success('保存成功');
    } catch (error) {
      toast.error(`保存失败: ${(error as Error).message}`);
    }
  });

  useEffect(() => {
    void loadState();
  }, []);

  if (loading) return <PageLoading loading />;

  return (
    <>
      <title>WebUI配置 - Liteyuki WebUI</title>

      <div className='flex flex-col gap-2'>
        <div className='flex items-center justify-between gap-3'>
          <div className='font-bold text-default-600 dark:text-default-400 px-1'>背景图</div>
        </div>
        <Controller
          control={control}
          name='backgroundImage'
          render={({ field }) => (
            <ImageInput
              {...field}
            />
          )}
        />
      </div>

      <div className='flex flex-col gap-2'>
        <div className='font-bold text-default-600 dark:text-default-400 px-1'>自定义图标</div>
        {siteConfig.navItems.map((item) => (
          <Controller
            key={item.label}
            control={control}
            name={`customIcons.${item.label}`}
            render={({ field }) => (
              <ImageInput
                {...field}
                label={item.label}
              />
            )}
          />
        ))}
      </div>

      <div className='flex flex-col gap-3 rounded-2xl border border-default-200/70 bg-default-50/60 p-4 dark:border-white/10 dark:bg-white/5'>
        <div className='flex items-center justify-between gap-3'>
          <div>
            <div className='text-sm font-semibold text-default-700 dark:text-default-200'>WebUI 登录</div>
          </div>
          <div className='flex flex-wrap justify-end gap-2'>
            <Chip
              size='sm'
              variant='flat'
              color={authState?.tokenLoginEnabled ? 'secondary' : 'default'}
            >
              {authState?.tokenLoginEnabled ? '临时 Token 已启用' : '临时 Token 已关闭'}
            </Chip>
            <Chip
              size='sm'
              variant='flat'
              color={authState?.passwordConfigured ? 'success' : 'warning'}
            >
              {authState?.passwordConfigured ? '已设置登录密码' : '未设置登录密码'}
            </Chip>
          </div>
        </div>
        <div className='flex flex-wrap gap-2'>
          <Button
            size='sm'
            color='primary'
            variant='flat'
            onPress={() => navigate('/config?tab=token')}
          >
            管理登录密码
          </Button>
          <Button
            size='sm'
            variant='light'
            onPress={() => void loadState(true)}
          >
            刷新状态
          </Button>
        </div>
      </div>

      <SaveButtons
        onSubmit={onSubmit}
        reset={() => void loadState()}
        isSubmitting={isSubmitting}
      />
    </>
  );
};

export default WebUIConfigCard;
