import { useEffect, useState } from 'react';
import { Controller, useForm } from 'react-hook-form';
import toast from 'react-hot-toast';

import SaveButtons from '@/components/button/save_buttons';
import PageLoading from '@/components/page_loading';
import SwitchCard from '@/components/switch_card';

import LlmManager from '@/controllers/llm_manager';
import QQManager from '@/controllers/qq_manager';

interface CoreFormData {
  fileLog: boolean;
  consoleLog: boolean;
  autoTimeSync: boolean;
  llmEnabled: boolean;
}

const CoreConfigCard = () => {
  const [loading, setLoading] = useState(true);
  const {
    control,
    handleSubmit,
    formState: { isSubmitting },
    setValue,
  } = useForm<CoreFormData>();

  const loadConfig = async (showTip = false) => {
    try {
      setLoading(true);
      const [config, llmSettings] = await Promise.all([
        QQManager.getAccountRuntimeConfig(),
        LlmManager.getSettings(),
      ]);
      setValue('fileLog', config.fileLog ?? false);
      setValue('consoleLog', config.consoleLog ?? true);
      setValue('autoTimeSync', config.autoTimeSync ?? true);
      setValue('llmEnabled', llmSettings.enabled ?? false);
      if (showTip) toast.success('刷新成功');
    } catch (error) {
      const msg = (error as Error).message;
      toast.error(`获取配置失败: ${msg}`);
    } finally {
      setLoading(false);
    }
  };

  const onSubmit = handleSubmit(async (data) => {
    try {
      await QQManager.setAccountRuntimeConfig({
        fileLog: data.fileLog,
        consoleLog: data.consoleLog,
        autoTimeSync: data.autoTimeSync,
      });
      try {
        await LlmManager.updateEnabled(data.llmEnabled);
      } catch (llmError) {
        throw new Error(`运行时配置已保存，但 LLM 开关保存失败: ${(llmError as Error).message}`);
      }
      toast.success('保存成功');
    } catch (error) {
      const msg = (error as Error).message;
      toast.error(`保存失败: ${msg}`);
    }
  });

  const onReset = () => {
    loadConfig();
  };

  const onRefresh = async () => {
    await loadConfig(true);
  };

  useEffect(() => {
    loadConfig();
  }, []);

  if (loading) return <PageLoading loading />;

  return (
    <>
      <title>核心配置 - Liteyuki WebUI</title>
      <div className='flex flex-col gap-1 mb-2'>
        <h3 className='text-lg font-semibold text-default-700'>Liteyuki 核心配置</h3>
        <p className='text-sm text-default-500'>
          控制 Liteyuki 核心能力，以及当前运行实例的基础行为。
        </p>
      </div>
      <div className='grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3'>
        <Controller
          control={control}
          name='llmEnabled'
          render={({ field }) => (
            <SwitchCard
              {...field}
              label='启用模型能力'
              description='控制 LLM 对话与能力面板是否可用'
            />
          )}
        />
        <Controller
          control={control}
          name='autoTimeSync'
          render={({ field }) => (
            <SwitchCard
              {...field}
              label='自动对时'
              description='自动校验并矫正系统时间偏差'
            />
          )}
        />
        <Controller
          control={control}
          name='fileLog'
          render={({ field }) => (
            <SwitchCard
              {...field}
              label='文件日志'
              description='是否将登录后日志写入到本地文件'
            />
          )}
        />
        <Controller
          control={control}
          name='consoleLog'
          render={({ field }) => (
            <SwitchCard
              {...field}
              label='控制台日志'
              description='是否在终端标准输出显示日志'
            />
          )}
        />
      </div>
      <SaveButtons
        onSubmit={onSubmit}
        reset={onReset}
        isSubmitting={isSubmitting}
        refresh={onRefresh}
      />
    </>
  );
};

export default CoreConfigCard;
