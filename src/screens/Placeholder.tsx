// Screens not built yet show their fixed title only. Replaced slice by slice.
export function Placeholder({ title }: { title: string }) {
  return <h1 className="m-0 text-[44px] font-bold leading-none tracking-[-0.05em]">{title}</h1>;
}
