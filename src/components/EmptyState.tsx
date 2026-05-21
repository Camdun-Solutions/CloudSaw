import type { ReactNode } from "react";

type EmptyStateProps = {
  title: string;
  body?: string;
  action?: ReactNode;
};

export default function EmptyState({ title, body, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center rounded-card border border-dashed border-saw-grey-300 bg-saw-white px-6 py-12 text-center">
      <h3 className="text-h2 font-semibold text-saw-grey-900">{title}</h3>
      {body ? (
        <p className="mt-2 max-w-md text-body text-saw-grey-600">{body}</p>
      ) : null}
      {action ? <div className="mt-5">{action}</div> : null}
    </div>
  );
}
