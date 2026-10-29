import {
  Suspense,
  lazy,
  useState,
  type ComponentType,
  type LazyExoticComponent,
  type ReactNode,
} from "react";
import { Loader2 } from "lucide-react";

/**
 * React.lazy 的具名导出版本：`lazyNamed(() => import("./X"), "X")`
 * 让按需加载的模块不必额外提供 default 导出。
 */
export function lazyNamed<
  // 与 React.lazy 自身的 ComponentType<any> 约束保持一致
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  M extends Record<K, ComponentType<any>>,
  K extends keyof M & string,
>(loader: () => Promise<M>, name: K): LazyExoticComponent<M[K]> {
  return lazy(() => loader().then((mod) => ({ default: mod[name] })));
}

/** 视图分片加载期间的占位 */
export function LazyViewFallback() {
  return (
    <div
      className="flex h-full min-h-[160px] items-center justify-center text-muted-foreground"
      data-testid="lazy-view-fallback"
    >
      <Loader2 className="h-5 w-5 animate-spin" />
    </div>
  );
}

export interface LazyMountProps {
  /** 首次为 true 时才挂载子节点（及其懒加载分片），之后保持挂载以保留关闭动画 */
  when: boolean;
  fallback?: ReactNode;
  children: ReactNode;
}

/**
 * 延迟挂载：对话框等默认关闭的重型组件在首次打开前不加载其代码分片。
 * 一旦打开过就保持挂载，由子组件自身的 open 属性控制显示与退出动画。
 */
export function LazyMount({ when, fallback = null, children }: LazyMountProps) {
  const [mounted, setMounted] = useState(when);
  if (when && !mounted) {
    setMounted(true);
  }
  if (!mounted && !when) {
    return null;
  }
  return <Suspense fallback={fallback}>{children}</Suspense>;
}
