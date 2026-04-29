import { Button } from '@heroui/button';
import { Textarea } from '@heroui/input';
import { useEffect, useState } from 'react';
import toast from 'react-hot-toast';
import { LuDownload, LuRefreshCw, LuUpload } from 'react-icons/lu';

import PageLoading from '@/components/page_loading';

import WebUIManager from '@/controllers/webui_manager';

async function saveConfigToFile (fileName: string, content: string) {
  const picker = (window as {
    showSaveFilePicker?: (options: {
      suggestedName: string;
      types: Array<{
        description: string;
        accept: Record<string, string[]>;
      }>;
    }) => Promise<{
      createWritable: () => Promise<{
        write: (data: string) => Promise<void>;
        close: () => Promise<void>;
      }>;
    }>;
  }).showSaveFilePicker;

  if (picker) {
    const handle = await picker({
      suggestedName: fileName,
      types: [
        {
          description: 'Liteyuki config',
          accept: {
            'text/plain': ['.yaml', '.yml', '.toml'],
          },
        },
      ],
    });
    const writable = await handle.createWritable();
    await writable.write(content);
    await writable.close();
    return;
  }

  const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
  const url = window.URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  window.URL.revokeObjectURL(url);
}

const BackupConfigCard: React.FC = () => {
  const [loading, setLoading] = useState(true);
  const [configState, setConfigState] = useState<ActiveAppConfigState | null>(null);

  const loadConfig = async (showTip = false) => {
    try {
      setLoading(true);
      const next = await WebUIManager.getActiveAppConfig();
      setConfigState(next);
      if (showTip) {
        toast.success('刷新成功');
      }
    } catch (error) {
      toast.error(`读取配置失败: ${(error as Error).message}`);
    } finally {
      setLoading(false);
    }
  };

  const handleExportConfig = async () => {
    if (!configState) {
      return;
    }

    try {
      const fileName = configState.configPath.split(/[\\/]/).pop() || 'config.yaml';
      await saveConfigToFile(fileName, configState.content);
      toast.success('配置文件已导出');
    } catch (error) {
      toast.error(`导出失败: ${(error as Error).message}`);
    }
  };

  const handleImportConfig = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;

    try {
      const content = await file.text();
      const next = await WebUIManager.replaceActiveAppConfig(content);
      setConfigState(next);
      toast.success('配置文件已替换，部分设置需要重启后生效');
    } catch (error) {
      toast.error(`导入失败: ${(error as Error).message}`);
    } finally {
      event.target.value = '';
    }
  };

  useEffect(() => {
    void loadConfig();
  }, []);

  if (loading) {
    return <PageLoading loading />;
  }

  return (
    <div className='space-y-5'>
      <div className='space-y-2'>
        <h3 className='text-lg font-medium'>备份与恢复</h3>
        <p className='text-sm text-default-500'>
          当前主配置文件。
        </p>
      </div>

      <div className='rounded-2xl border border-default-200/70 bg-default-50/60 p-4 dark:border-white/10 dark:bg-white/5'>
        <div className='text-xs text-default-400'>配置路径</div>
        <div className='mt-1 break-all font-mono text-sm text-default-700 dark:text-default-200'>
          {configState?.configPath || '-'}
        </div>
      </div>

      <div className='flex flex-wrap gap-3'>
        <label className='cursor-pointer'>
          <input
            type='file'
            accept='.yaml,.yml,.toml'
            onChange={handleImportConfig}
            className='hidden'
          />
          <Button
            as='span'
            color='secondary'
            variant='flat'
            startContent={<LuUpload size={16} />}
          >
            导入配置文件
          </Button>
        </label>
        <Button
          color='primary'
          variant='flat'
          startContent={<LuDownload size={16} />}
          onPress={handleExportConfig}
        >
          导出配置文件
        </Button>
        <Button
          variant='light'
          startContent={<LuRefreshCw size={16} />}
          onPress={() => void loadConfig(true)}
        >
          刷新
        </Button>
      </div>

      <Textarea
        label='主配置预览'
        value={configState?.content || ''}
        readOnly
        minRows={16}
        variant='bordered'
        classNames={{
          input: 'font-mono text-sm',
          inputWrapper: 'bg-default-100/40 dark:bg-white/5',
        }}
      />

      <div className='rounded-2xl border border-warning/30 bg-warning/10 p-4 text-sm text-warning-700 dark:text-warning-300'>
        导入会直接覆盖当前主配置文件。
      </div>
    </div>
  );
};

export default BackupConfigCard;
