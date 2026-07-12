import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import HomepageFeatures from '@site/src/components/HomepageFeatures';

import styles from './index.module.css';

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();

  return (
    <header className={styles.hero}>
      <div className={styles.heroBackdrop} />
      <div className={styles.heroInner}>
        <div className={styles.heroContent}>
          <p className={styles.heroKicker}>HeliOS documentation</p>
          <Heading as="h1" className={styles.heroTitle}>
            {siteConfig.title}
          </Heading>
          <p className={styles.heroSubtitle}>
            Practical docs for running streams, authoring pipelines, and debugging devices in the field.
          </p>
          <div className={styles.heroPills}>
            <Link className={styles.heroPill} to="/getting-started/install">
              Get online
            </Link>
            <Link className={styles.heroPill} to="/os/devices/streams/overview">
              Streams
            </Link>
            <Link className={styles.heroPill} to="/os/pipelines/overview">
              Pipelines
            </Link>
            <Link className={styles.heroPill} to="/release-notes">
              Release notes
            </Link>
          </div>
          <div className={styles.heroActions}>
            <Link className={styles.heroButtonPrimary} to="/getting-started/install">
              Install and First Boot
            </Link>
            <Link className={styles.heroButtonGhost} to="/guides/troubleshooting">
              Troubleshooting
            </Link>
          </div>
        </div>
        <div className={styles.heroPanel}>
          <div className={styles.heroPanelHeader}>
            <span className={styles.heroPanelKicker}>Live surfaces</span>
            <span className={styles.heroPanelHint}>Open these pages when something feels off.</span>
          </div>
          <div className={styles.heroPanelGrid}>
            <Link className={styles.heroStat} to="/os/devices/overview">
              <span className={styles.heroStatLabel}>Devices</span>
              <span className={styles.heroStatValue}>Streams, peripherals, capture health</span>
            </Link>
            <Link className={styles.heroStat} to="/os/pipelines/overview">
              <span className={styles.heroStatLabel}>Pipelines</span>
              <span className={styles.heroStatValue}>Deploy, validate, tune per stream</span>
            </Link>
            <Link className={styles.heroStat} to="/os/systems/overview">
              <span className={styles.heroStatLabel}>Systems</span>
              <span className={styles.heroStatValue}>Logs, console, processes, sensors</span>
            </Link>
          </div>
          <div className={styles.heroPanelFooter}>
            <span className={styles.heroPanelFooterLabel}>Fast path:</span>
            <Link className={styles.heroPanelFooterLink} to="/guides/first-login-and-sanity-check">
              sanity check
            </Link>
            <span className={styles.heroPanelFooterDot} aria-hidden="true">
              ·
            </span>
            <Link className={styles.heroPanelFooterLink} to="/guides/get-online">
              get online
            </Link>
            <span className={styles.heroPanelFooterDot} aria-hidden="true">
              ·
            </span>
            <Link className={styles.heroPanelFooterLink} to="/os/pipelines/overview">
              pipelines
            </Link>
          </div>
          <div className={styles.heroPulse} aria-hidden="true" />
        </div>
      </div>
    </header>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout description="HeliOS documentation for devices, streams, pipelines, troubleshooting, and API integration.">
      <HomepageHeader />
      <main className={styles.main}>
        <HomepageFeatures />
      </main>
    </Layout>
  );
}
