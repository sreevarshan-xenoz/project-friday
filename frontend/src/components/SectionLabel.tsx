import './SectionLabel.css';

interface SectionLabelProps {
  children: string;
}

export function SectionLabel({ children }: SectionLabelProps) {
  return <h3 className="fr-section-label">{children.toUpperCase()}</h3>;
}
