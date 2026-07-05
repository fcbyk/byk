import { Layout as BaseLayout } from '@rspress/core/theme-original';
import type { LayoutProps } from '@rspress/core/theme-original';
import { BykHome } from './components/Home';
import './index.css';

/**
 * 自定义首页：完全自己写组件，不依赖 frontmatter 的 hero / features。
 *
 * rspress 在 pageType === 'home' 时会调用 LayoutProps.HomeLayout，
 * 我们把它替换成 BykHome 即可保留 Nav / 全局样式的同时丢掉自动生成。
 */
function HomeLayout() {
  return <BykHome />;
}

/**
 * 透传所有 LayoutProps，确保 Nav / Logo / 主题色 / 国际化等都正常工作。
 * 只覆盖 HomeLayout，其余交还给原版 Layout 处理。
 */
function Layout(props: LayoutProps) {
  return <BaseLayout {...props} HomeLayout={HomeLayout} />;
}

export { Layout, BykHome };
export * from '@rspress/core/theme-original';
