import { useEffect, useState } from "react";
import { Button } from "antd";

export default function () {
    const [config, set_config] = useState("");
    const [config_file, set_config_status] = useState<any>({});

    const refresh = async function () {
        set_config(await (await fetch("/api/config")).text());
        set_config_status(await (await fetch("/api/config/status")).json());
    }

    useEffect(() => {
        refresh();
    }, []);

    return (
        <div>
            <h1>Time: {new Date(config_file.mtimeMs).toLocaleString()}</h1>
            <Button onClick={refresh}>刷新</Button>
            <pre>{config}</pre>
        </div>
    )
}