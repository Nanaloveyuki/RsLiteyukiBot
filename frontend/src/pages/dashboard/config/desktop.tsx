import { useRequest } from 'ahooks';
import { useEffect } from 'react';
import { Controller, useForm } from 'react-hook-form';
import toast from 'react-hot-toast';

import SaveButtons from '@/components/button/save_buttons';
import PageLoading from '@/components/page_loading';
import SwitchCard from '@/components/switch_card';

import WebUIManager from '@/controllers/webui_manager';

interface DesktopFormData {
  closeToTray: boolean;
}

const DesktopConfigCard = () => {
  const {
    data: settings,
    loading,
    error,
    refreshAsync,
  } = useRequest(WebUIManager.getDesktopSettings);

  const {
    control,
    handleSubmit,
    formState: { isSubmitting },
    setValue,
  } = useForm<DesktopFormData>({
    defaultValues: {
      closeToTray: true,
    },
  });

  const reset = () => {
    if (settings) {
      setValue('closeToTray', settings.closeToTray);
    }
  };

  const onSubmit = handleSubmit(async (data) => {
    try {
      const nextSettings = await WebUIManager.updateDesktopSettings(data);
      setValue('closeToTray', nextSettings.closeToTray);
      toast.success('保存成功');
    } catch (saveError) {
      const msg = (saveError as Error).message;
      toast.error(`保存失败: ${msg}`);
    }
  });

  const onRefresh = async () => {
    try {
      const nextSettings = await refreshAsync();
      setValue('closeToTray', nextSettings.closeToTray);
      toast.success('刷新成功');
    } catch (refreshError) {
      const msg = (refreshError as Error).message;
      toast.error(`刷新失败: ${msg}`);
    }
  };

  useEffect(() => {
    reset();
  }, [settings]);

  if (loading) return <PageLoading loading />;

  return (
    <>
      <title>桌面配置 - Liteyuki WebUI</title>
      <div className='flex flex-col gap-1 mb-2'>
        <h3 className='text-lg font-semibold text-default-700'>桌面行为</h3>
        <p className='text-sm text-default-500'>
          控制 Tauri 桌面端关闭窗口时的后台运行策略。
        </p>
      </div>
      <div className='grid grid-cols-1 md:grid-cols-2 gap-3'>
        <Controller
          control={control}
          name='closeToTray'
          render={({ field }) => (
            <SwitchCard
              value={field.value}
              onValueChange={(value: boolean) => field.onChange(value)}
              disabled={!!error}
              label='关闭时后台运行'
              description='关闭桌面窗口后保留后端服务，Windows 放入托盘，macOS/Linux 保持后台运行'
            />
          )}
        />
      </div>
      <SaveButtons
        onSubmit={onSubmit}
        reset={reset}
        isSubmitting={isSubmitting || loading}
        refresh={onRefresh}
      />
    </>
  );
};

export default DesktopConfigCard;
