import { LogLevel } from '@/const/enum';

export const gradientText = (
  text: string,
  startColor: [number, number, number] = [255, 0, 0],
  endColor: [number, number, number] = [0, 0, 255],
  bold: boolean = false,
  italic: boolean = false,
  underline: boolean = false
) => {
  const steps = text.length;
  const colorStep = startColor.map(
    (start, index) => (endColor[index] - start) / steps
  );

  let coloredText = '';
  for (let i = 0; i < steps; i++) {
    const color = startColor.map((start, index) =>
      Math.round(start + colorStep[index] * i)
    );
    coloredText += `\x1b[38;2;${color[0]};${color[1]};${color[2]}m${text[i]}`;
  }

  // 添加样式
  if (bold) {
    coloredText = `\x1b[1m${coloredText}`;
  }
  if (italic) {
    coloredText = `\x1b[3m${coloredText}`;
  }
  if (underline) {
    coloredText = `\x1b[4m${coloredText}`;
  }

  return coloredText + '\x1b[0m'; // 重置颜色和样式
};

export const logColor = {
  [LogLevel.DEBUG]: 'green',
  [LogLevel.INFO]: 'black',
  [LogLevel.WARN]: 'yellow',
  [LogLevel.ERROR]: 'red',
  [LogLevel.FATAL]: 'red',
} as const;

export const parseLogLevel = (content: string, fallback: LogLevel = LogLevel.INFO): LogLevel => {
  const bracketLevel = content.match(/\[([a-zA-Z]+)\]/)?.[1];
  const pipeLevel = content.match(/\|\s*([A-Z]+)\s+/)?.[1];
  const rawLevel = bracketLevel ?? pipeLevel;
  const normalized = rawLevel?.toLowerCase();

  switch (normalized) {
    case LogLevel.DEBUG:
      return LogLevel.DEBUG;
    case LogLevel.INFO:
      return LogLevel.INFO;
    case LogLevel.WARN:
    case 'warning':
      return LogLevel.WARN;
    case LogLevel.ERROR:
      return LogLevel.ERROR;
    case LogLevel.FATAL:
      return LogLevel.FATAL;
    default:
      return fallback;
  }
};

export const colorizeLogLevel = (content: string) => {
  let _content = content;
  const level = parseLogLevel(content);
  const color = logColor[level];
  switch (color) {
    case 'green':
      _content = `\x1b[32m${_content}\x1b[0m`;
      break;
    case 'black':
      _content = `\x1b[30m${_content}\x1b[0m`;
      break;
    case 'yellow':
      _content = `\x1b[33m${_content}\x1b[0m`;
      break;
    case 'red':
      _content = `\x1b[31m${_content}\x1b[0m`;
      break;
    default:
      _content = `\x1b[30m${_content}\x1b[0m`;
  }
  return {
    content: _content,
    level,
  };
};

export const colorizeLogLevelWithTag = (content: string, level: LogLevel) => {
  let _content = content;
  switch (level) {
    case LogLevel.DEBUG:
      _content = `\x1b[32m[DEBUG] ${content}\x1b[0m`;
      break;
    case LogLevel.INFO:
      _content = `\x1b[30m[INFO] ${content}\x1b[0m`;
      break;
    case LogLevel.WARN:
      _content = `\x1b[33m[WARN] ${content}\x1b[0m`;
      break;
    case LogLevel.ERROR:
      _content = `\x1b[31m[ERROR] ${content}\x1b[0m`;
      break;
    case LogLevel.FATAL:
      _content = `\x1b[31m[FATAL] ${content}\x1b[0m`;
      break;
    default:
      _content = `\x1b[30m${content}\x1b[0m`;
  }
  return _content;
};
