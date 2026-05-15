package oris

import (
	"fmt"

	"github.com/Colin4k1024/Oris/sdks/go/experience"
	"github.com/Colin4k1024/Oris/sdks/go/hub"
	"github.com/Colin4k1024/Oris/sdks/go/store"
	orissync "github.com/Colin4k1024/Oris/sdks/go/sync"
)

type HubSync struct {
	BaseURL string
	APIKey  string
	Seed    [32]byte
	NodeID  string
}

type ExperienceSync struct {
	BaseURL  string
	APIKey   string
	Seed     [32]byte
	SenderID string
}

type MySQLSync struct {
	Host     string
	Port     int
	User     string
	Password string
	Database string
}

type Config struct {
	StorePath  string
	MySQL      *MySQLSync
	Hub        *HubSync
	Experience *ExperienceSync
}

type OrisClient struct {
	Store store.Store
	Sync  *orissync.SyncManager
}

func NewClient(cfg Config) (*OrisClient, error) {
	var s store.Store
	var err error

	if cfg.MySQL != nil {
		s, err = store.OpenMySQL(store.MySQLConfig{
			Host:     cfg.MySQL.Host,
			Port:     cfg.MySQL.Port,
			User:     cfg.MySQL.User,
			Password: cfg.MySQL.Password,
			Database: cfg.MySQL.Database,
		})
		if err != nil {
			return nil, fmt.Errorf("open mysql store: %w", err)
		}
	} else {
		storePath := cfg.StorePath
		if storePath == "" {
			storePath = "oris_genes.db"
		}
		s, err = store.Open(storePath)
		if err != nil {
			return nil, fmt.Errorf("open local store: %w", err)
		}
	}

	syncCfg := orissync.Config{Store: s}

	if cfg.Experience != nil {
		syncCfg.Experience = experience.NewClient(experience.Config{
			BaseURL:  cfg.Experience.BaseURL,
			APIKey:   cfg.Experience.APIKey,
			Seed:     cfg.Experience.Seed,
			SenderID: cfg.Experience.SenderID,
		})
	}

	if cfg.Hub != nil {
		syncCfg.Hub = hub.NewClient(hub.Config{
			BaseURL: cfg.Hub.BaseURL,
			APIKey:  cfg.Hub.APIKey,
			Seed:    cfg.Hub.Seed,
			NodeID:  cfg.Hub.NodeID,
		})
	}

	return &OrisClient{
		Store: s,
		Sync:  orissync.New(syncCfg),
	}, nil
}

func (c *OrisClient) Close() error {
	return c.Store.Close()
}
