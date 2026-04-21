export default function PageBackground() {
  return (
    <div className="pointer-events-none fixed inset-0 -z-10 h-full w-full overflow-hidden bg-gradient-to-br from-cyan-50 via-white to-sky-50 dark:from-slate-950 dark:via-slate-950 dark:to-slate-900">
      <div className="absolute left-[-10%] top-[-10%] h-[30rem] w-[30rem] rounded-full bg-cyan-200/36 blur-[100px] dark:bg-cyan-400/12" />
      <div className="absolute right-[-10%] top-[14%] h-[24rem] w-[24rem] rounded-full bg-sky-200/34 blur-[92px] dark:bg-sky-400/12" />
      <div className="absolute bottom-[-12%] left-[18%] h-[34rem] w-[34rem] rounded-full bg-cyan-100/58 blur-[108px] dark:bg-cyan-300/8" />
    </div>
  );
}
