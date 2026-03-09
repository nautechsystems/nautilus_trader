import { useEffect, type ReactNode } from 'react';

type Props = {
  title: string;
  children: ReactNode;
};

export default function Title({ title, children }: Props) {
  useEffect(() => {
    document.title = `flux - ${title}`;
  }, [title]);

  return <>{children}</>;
}
