import React, { Component } from 'react';

import {Button, Dialog, DialogActions, DialogContent} from "@mui/material";

export class CriticalButton extends Component {

    constructor(props) {
        super(props);
        this.state = { open: false }
    }

    __open = () => {
        this.setState({ open: true })
    }

    __close = () => {
        this.setState({ open: false })
    }

    __confirmed = () => {
        this.setState({ open: false })
        this.props.onConfirmed();
    }

    render() {

        let { open } = this.state;

        return (
            <>
                <Button { ...this.props } onClick={this.__open}>{this.props.children}</Button>
                <Dialog open={open}>
                    <DialogContent>{ this.props.dialog?.content || "Are you sure?" }</DialogContent>
                    <DialogActions>
                        <Button autoFocus onClick={this.__close} variant={"outlined"} color="default">
                            Cancel
                        </Button>
                        <Button onClick={this.__confirmed} variant={"outlined"} color="primary">
                            Yes
                        </Button>
                    </DialogActions>
                </Dialog>
            </>
        );
    }
}
